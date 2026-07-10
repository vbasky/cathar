//! The Cathar spectral editor — RX-style layout and chrome.

use crate::axes::{self, FREQ_AXIS_W, TIME_AXIS_H};
use crate::engine::{Engine, Monitor};
use crate::histogram::LevelHistogram;
use crate::icons::{
    self, channel_chip, rich, toolbar_button, toolbar_toggle, transport_play_button,
};
use crate::native_menu::{self, NativeMenu};
use crate::panel::{
    self, ValueFmt, action_row, check_row, compact_row, compare_render_row, hint, param_f32,
    param_u32, param_usize, prepare_module, render_button, render_button_enabled, secondary_button,
    section, side_section, square_checkbox, stem_chips, tool_group, tool_tile, vertical_fader,
    vu_meter_h,
};
use crate::spectral_edit::{Selection, SpectralOp, apply_spectral};
use crate::spectro::{
    ChannelView, channel_samples, colorize, compute_spectrogram, is_stereo, stack_vertical,
    waveform_envelope,
};
use crate::theme::{self, Appearance};
use crate::visualizer::SpectrumViz;
use cathar::{
    AudioData, Denoiser, EnhanceMethod, NoisePrint, SpectralDenoiser, Spectrogram, StretchMode,
};
use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Theme, pos2};

const FFT_SIZE: usize = 2048;
const HOP: usize = 512;
/// Max spectrogram columns (GPU texture side limit is 16384; leave margin).
const MAX_COLS: usize = 16000;

/// Active floating module window (RX Modules list → panel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Module {
    Denoise,
    Dehum,
    Declick,
    Decrackle,
    Declip,
    Deess,
    Dereverb,
    Dewind,
    Deplosive,
    Derustle,
    Repair,
    VoiceIsolate,
    Breath,
    Enhance,
    Inpaint,
    Dewow,
    Azimuth,
    Align,
    Dequantize,
    Deemphasis,
    RiaaNormalize,
    Transform,
    Hpss,
    Sms,
    Selection,
    Equalizer,
}

/// Central pane: spectrogram editor, media playlist, or classic visualizer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ViewerMode {
    #[default]
    Spectrogram,
    Playlist,
    Visualizer,
}

/// One track in the session queue (path + display name).
#[derive(Clone)]
struct PlaylistEntry {
    path: std::path::PathBuf,
    name: String,
}

/// iTunes-style graphic EQ centres (Hz).
const EQ_BAND_HZ: [f32; 10] =
    [32.0, 64.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0];
const EQ_BAND_LABELS: [&str; 10] = ["32", "64", "125", "250", "500", "1K", "2K", "4K", "8K", "16K"];
const EQ_GAIN_MIN: f32 = -12.0;
const EQ_GAIN_MAX: f32 = 12.0;

#[derive(Clone, Copy)]
struct EqPresetDef {
    name: &'static str,
    blurb: &'static str,
    /// Gains (dB) for [`EQ_BAND_HZ`] — kept mild so cascades don’t clip.
    gains: [f32; 10],
}

/// Genre / use-case EQ starting points (10-band). Gains are conservative so
/// multi-band boosts stay under full-scale without needing peak-normalize.
const EQ_PRESETS: &[EqPresetDef] = &[
    EqPresetDef { name: "Flat", blurb: "No change — bypass curve", gains: [0.0; 10] },
    EqPresetDef {
        name: "Pop",
        blurb: "Slight bass + presence lift",
        gains: [2.0, 2.5, 1.0, -1.0, 0.0, 0.5, 1.5, 2.0, 1.5, 1.0],
    },
    EqPresetDef {
        name: "Jazz",
        blurb: "Warm lows, smooth air",
        gains: [2.0, 2.0, 0.5, -0.5, 0.0, 0.0, 0.5, 1.0, 1.5, 1.0],
    },
    EqPresetDef {
        name: "Rock",
        blurb: "Punchy low end + bite",
        gains: [3.5, 3.0, 1.0, -1.5, -1.0, 0.5, 2.0, 2.5, 2.0, 1.0],
    },
    EqPresetDef {
        name: "Classical",
        blurb: "Gentle air, natural balance",
        gains: [1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.5, 1.0],
    },
    EqPresetDef {
        name: "Vocal",
        blurb: "Clear speech / lead vocal",
        gains: [-2.0, -1.5, -1.0, -0.5, 0.5, 1.5, 2.5, 2.0, 1.5, 0.5],
    },
    EqPresetDef {
        name: "Bass+",
        blurb: "Low-end weight",
        gains: [4.0, 3.5, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    },
    EqPresetDef {
        name: "Treble+",
        blurb: "Air and brilliance",
        gains: [0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.5, 2.5, 3.0, 3.0],
    },
    EqPresetDef {
        name: "Podcast",
        blurb: "Dialogue focus, less mud",
        gains: [-3.0, -2.5, -1.5, -1.0, 0.5, 1.5, 2.5, 2.0, 1.0, 0.0],
    },
    EqPresetDef {
        name: "Loudness",
        blurb: "Mild smile curve (was “Perfect” — those +9…+11 dB settings clipped hard)",
        gains: [3.0, 2.5, 1.5, 0.5, 0.0, 0.5, 1.5, 2.5, 3.0, 2.5],
    },
];

/// Apply 10-band graphic EQ (RBJ shelves + proportional-Q peaking, f64 design).
fn apply_eq_gains(signal: &[f32], sample_rate: u32, gains: &[f32; 10], preamp_db: f32) -> Vec<f32> {
    cathar::graphic_eq(signal, sample_rate, &EQ_BAND_HZ, gains, preamp_db)
}

fn eq_is_flat(gains: &[f32; 10], preamp: f32) -> bool {
    preamp.abs() < 0.05 && gains.iter().all(|g| g.abs() < 0.05)
}

/// Parse a simple M3U / M3U8 into absolute paths (relative entries resolve vs list file).
fn parse_m3u(path: &std::path::Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let text = std::fs::read_to_string(path)?;
    let base = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Skip extended HTTP streams for now — local files only.
        if line.starts_with("http://") || line.starts_with("https://") {
            continue;
        }
        let p = std::path::PathBuf::from(line);
        let abs = if p.is_absolute() { p } else { base.join(p) };
        if abs.is_file() {
            out.push(abs);
        }
    }
    Ok(out)
}

impl Module {
    fn title(self) -> &'static str {
        match self {
            Self::Denoise => "Spectral De-noise",
            Self::Dehum => "De-hum",
            Self::Declick => "De-click",
            Self::Decrackle => "De-crackle",
            Self::Declip => "De-clip",
            Self::Deess => "De-ess",
            Self::Dereverb => "De-reverb",
            Self::Dewind => "De-wind",
            Self::Deplosive => "De-plosive",
            Self::Derustle => "De-rustle",
            Self::Repair => "Spectral Repair",
            Self::VoiceIsolate => "Dialogue Isolate",
            Self::Breath => "Breath Control",
            Self::Enhance => "Bandwidth Extend",
            Self::Inpaint => "Interpolate",
            Self::Dewow => "Wow & Flutter",
            Self::Azimuth => "Azimuth",
            Self::Align => "Align",
            Self::Dequantize => "Dequantize",
            Self::Deemphasis => "De-emphasis",
            Self::RiaaNormalize => "RIAA / Normalize",
            Self::Transform => "Tempo / Pitch / Speed",
            Self::Hpss => "Separate (HPSS)",
            Self::Sms => "Tonal Purify (SMS)",
            Self::Selection => "Spectral Selection",
            Self::Equalizer => "Equalizer",
        }
    }

    /// One-line purpose — keeps the catalogue scannable.
    fn blurb(self) -> &'static str {
        match self {
            Self::Denoise => "Hiss, broadband noise, noise prints",
            Self::Dehum => "Mains buzz 50/60 Hz + harmonics",
            Self::Declick => "Isolated ticks and dropouts",
            Self::Decrackle => "Dense vinyl surface noise",
            Self::Declip => "Rebuild flattened peaks",
            Self::Deess => "Tame harsh sibilance",
            Self::Dereverb => "Room tails · WPE option",
            Self::Dewind => "Low rumble / wind high-pass",
            Self::Deplosive => "Mic pops on plosives",
            Self::Derustle => "Clothing / lav rustle",
            Self::Repair => "Paint out spectral glitches",
            Self::VoiceIsolate => "Keep speech, gate the rest",
            Self::Breath => "Soften pre-speech breaths",
            Self::Enhance => "Restore missing high band",
            Self::Inpaint => "Fill gaps and mutes",
            Self::Dewow => "Tape pitch wander",
            Self::Azimuth => "Stereo head skew",
            Self::Align => "Multi-mic time align",
            Self::Dequantize => "Low-bit lattice lift",
            Self::Deemphasis => "FM / CD playback curves",
            Self::RiaaNormalize => "Vinyl EQ + loudness",
            Self::Transform => "Time-stretch and pitch",
            Self::Hpss => "Harmonic vs percussive",
            Self::Sms => "Keep tonals, drop residual",
            Self::Selection => "Gain / heal a drawn region",
            Self::Equalizer => "Genre / use-case tone curves",
        }
    }

    /// Phosphor glyph for the toolbox list (RX-style icon column).
    fn icon(self) -> &'static str {
        match self {
            Self::Denoise => icons::WAVEFORM,
            Self::Dehum => icons::EQUALIZER,
            Self::Declick => icons::LIGHTNING,
            Self::Decrackle => icons::SPARKLE,
            Self::Declip => icons::GAUGE,
            Self::Deess => icons::SPEAKER_SLASH,
            Self::Dereverb => icons::EAR,
            Self::Dewind => icons::WIND,
            Self::Deplosive => icons::MICROPHONE,
            Self::Derustle => icons::BROOM,
            Self::Repair => icons::WRENCH,
            Self::VoiceIsolate => icons::MICROPHONE_STAGE,
            Self::Breath => icons::WIND,
            Self::Enhance => icons::ARROWS_OUT_LINE_HORIZONTAL,
            Self::Inpaint => icons::DROP,
            Self::Dewow => icons::CLOCK_CLOCKWISE,
            Self::Azimuth => icons::COMPASS,
            Self::Align => icons::ARROWS_OUT_LINE_HORIZONTAL,
            Self::Dequantize => icons::FADERS,
            Self::Deemphasis => icons::DISC,
            Self::RiaaNormalize => icons::DISC,
            Self::Transform => icons::FADERS,
            Self::Hpss => icons::MUSIC_NOTES,
            Self::Sms => icons::MUSIC_NOTE,
            Self::Selection => icons::MAGNIFYING_GLASS,
            Self::Equalizer => icons::EQUALIZER,
        }
    }

    fn matches_filter(self, q: &str) -> bool {
        if q.is_empty() {
            return true;
        }
        let q = q.to_ascii_lowercase();
        self.title().to_ascii_lowercase().contains(&q)
            || self.blurb().to_ascii_lowercase().contains(&q)
    }
}

/// Workflow-ordered catalogue groups (not a flat dump of 25 tools).
struct ToolGroupDef {
    id: &'static str,
    icon: &'static str,
    title: &'static str,
    subtitle: &'static str,
    accent: Color32,
    default_open: bool,
    tools: &'static [Module],
}

const TOOL_GROUPS: &[ToolGroupDef] = &[
    ToolGroupDef {
        id: "grp_noise",
        icon: icons::WAVEFORM,
        title: "Noise & hum",
        subtitle: "Broadband · mains · surface",
        accent: Color32::from_rgb(0, 214, 160),
        default_open: true,
        tools: &[Module::Denoise, Module::Dehum, Module::Decrackle, Module::Dewind],
    },
    ToolGroupDef {
        id: "grp_damage",
        icon: icons::WRENCH,
        title: "Damage repair",
        subtitle: "Clicks · clips · holes · glitches",
        accent: Color32::from_rgb(255, 148, 48),
        default_open: true,
        tools: &[Module::Declick, Module::Declip, Module::Repair, Module::Inpaint],
    },
    ToolGroupDef {
        id: "grp_dialogue",
        icon: icons::MICROPHONE,
        title: "Dialogue",
        subtitle: "Voice, sibilance, breath, lav",
        accent: Color32::from_rgb(64, 160, 255),
        default_open: false,
        tools: &[
            Module::VoiceIsolate,
            Module::Deess,
            Module::Breath,
            Module::Deplosive,
            Module::Derustle,
        ],
    },
    ToolGroupDef {
        id: "grp_space",
        icon: icons::WIND,
        title: "Space",
        subtitle: "Room and reverb tails",
        accent: Color32::from_rgb(168, 96, 255),
        default_open: false,
        tools: &[Module::Dereverb],
    },
    ToolGroupDef {
        id: "grp_media",
        icon: icons::MUSIC_NOTES,
        title: "Media & archive",
        subtitle: "Vinyl · tape · broadcast curves",
        accent: Color32::from_rgb(255, 100, 80),
        default_open: false,
        tools: &[
            Module::RiaaNormalize,
            Module::Deemphasis,
            Module::Dequantize,
            Module::Dewow,
            Module::Azimuth,
            Module::Align,
        ],
    },
    ToolGroupDef {
        id: "grp_shape",
        icon: icons::EQUALIZER,
        title: "Shape & separate",
        subtitle: "Stretch · pitch · layers · selection",
        accent: Color32::from_rgb(0, 220, 180),
        default_open: false,
        tools: &[
            Module::Selection,
            Module::Equalizer,
            Module::Enhance,
            Module::Transform,
            Module::Hpss,
            Module::Sms,
        ],
    },
];

/// User-adjustable parameters for each whole-file restoration stage.
#[derive(Clone)]
struct FxParams {
    // denoise
    denoise_alpha: f32,
    denoise_beta: f32,
    denoise_noise_ratio: f32,
    denoise_fft: usize,
    denoise_wiener: bool,
    denoise_coherent: bool,
    // dehum
    hum_freq: f32,
    hum_harmonics: usize,
    hum_adaptive: bool,
    // declick / decrackle / declip
    declick_threshold: f32,
    declick_window: usize,
    declip_threshold: f32,
    decrackle_sensitivity: f32,
    // deess
    deess_crossover: f32,
    deess_threshold_db: f32,
    deess_ratio: f32,
    deess_bands: usize,
    deess_multiband: bool,
    // dereverb
    dereverb_strength: f32,
    dereverb_wpe: bool,
    dereverb_taps: usize,
    dereverb_delay: usize,
    dereverb_iters: u32,
    // wind / plosive / rustle / repair
    dewind_cutoff: f32,
    deplosive_strength: f32,
    derustle_strength: f32,
    repair_strength: f32,
    // inpaint
    inpaint_max_gap_ms: f32,
    inpaint_iters: u32,
    // azimuth / align
    azimuth_max_ms: f32,
    align_max_ms: f32,
    // dequant / deemph / normalize / vinyl
    dequant_bits: u32,
    dequant_strength: f32,
    deemph_curve: cathar::Emphasis,
    normalize_lufs: f32,
    normalize_ceiling: f32,
    normalize_peak_db: f32,
    normalize_use_peak: bool,
    riaa_elliptical: bool,
    elliptical_hz: f32,
    // enhance / transform
    enhance_target_hz: u32,
    enhance_method: EnhanceMethod,
    hpss_kernel: usize,
    tempo_factor: f32,
    pitch_semitones: f32,
    speed_factor: f32,
    stretch_mode: StretchMode,
    // selection
    heal_strength: f32,
    // equalizer: last preset index (EQ_PRESETS.len() == Custom)
    eq_preset: usize,
    /// Live 10-band gains (dB); presets write these, faders edit them.
    eq_gains: [f32; 10],
    /// Overall preamp (dB), like iTunes.
    eq_preamp: f32,
    /// When true, EQ is applied to the monitoring path (hear it while playing).
    eq_enabled: bool,
}

impl Default for FxParams {
    fn default() -> Self {
        Self {
            denoise_alpha: 3.0,
            denoise_beta: 0.01,
            denoise_noise_ratio: 0.15,
            denoise_fft: 2048,
            denoise_wiener: false,
            denoise_coherent: false,
            hum_freq: 60.0,
            hum_harmonics: 4,
            hum_adaptive: false,
            declick_threshold: 5.0,
            declick_window: 64,
            declip_threshold: 0.95,
            decrackle_sensitivity: 5.0,
            deess_crossover: 6000.0,
            deess_threshold_db: -30.0,
            deess_ratio: 4.0,
            deess_bands: 4,
            deess_multiband: false,
            dereverb_strength: 0.5,
            dereverb_wpe: false,
            dereverb_taps: 15,
            dereverb_delay: 3,
            dereverb_iters: 3,
            dewind_cutoff: 80.0,
            deplosive_strength: 0.5,
            derustle_strength: 0.5,
            repair_strength: 0.5,
            inpaint_max_gap_ms: 50.0,
            inpaint_iters: 3,
            azimuth_max_ms: 5.0,
            align_max_ms: 50.0,
            dequant_bits: 16,
            dequant_strength: 0.7,
            deemph_curve: cathar::Emphasis::Fm50,
            normalize_lufs: -14.0,
            normalize_ceiling: -1.0,
            normalize_peak_db: -1.0,
            normalize_use_peak: false,
            riaa_elliptical: false,
            elliptical_hz: 200.0,
            enhance_target_hz: 48_000,
            enhance_method: EnhanceMethod::Replicate,
            hpss_kernel: 17,
            tempo_factor: 1.0,
            pitch_semitones: 0.0,
            speed_factor: 1.0,
            stretch_mode: StretchMode::Wsola,
            heal_strength: 1.0,
            eq_preset: 0,
            eq_gains: [0.0; 10],
            eq_preamp: 0.0,
            eq_enabled: true,
        }
    }
}

/// Top-level editor state.
pub(crate) struct CatharGui {
    engine: Option<Engine>,
    /// The pristine decode, kept for A/B compare.
    original: Option<AudioData>,
    /// Edit history; `history[hist_idx]` is the current buffer.
    history: Vec<AudioData>,
    hist_idx: usize,
    texture: Option<TextureHandle>,
    /// The bundled logo, shown as the empty-state splash.
    logo: Option<TextureHandle>,
    /// Cached STFT of the displayed buffer.
    spec: Option<Spectrogram>,
    sample_rate: u32,
    duration: f32,
    n_channels: usize,
    /// Waveform envelopes — L (or mid) and optional R for stereo overview.
    waveform_l: Vec<(f32, f32)>,
    waveform_r: Vec<(f32, f32)>,
    /// Spectrogram / overview channel view.
    channel_view: ChannelView,
    /// Playback routing (independent of display view).
    monitor: Monitor,
    /// Output gain 0…1.5 (engine linear volume).
    volume: f32,
    /// Current selection in physical units (seconds, Hz).
    selection: Option<Selection>,
    drag_anchor: Option<Pos2>,
    show_original: bool,
    gain_db: f32,
    /// Spectrogram display window (dB).
    db_floor: f32,
    db_ceil: f32,
    zoom_x: f32,
    zoom_y: f32,
    fx: FxParams,
    appearance: Appearance,
    resolved_theme: Theme,
    status: String,
    /// Open floating module (RX Modules panel).
    active_module: Option<Module>,
    /// Filter text for the modules catalogue.
    module_filter: String,
    /// Last loaded basename (window title).
    file_name: Option<String>,
    /// Full path of the last opened file (for playlist re-open).
    file_path: Option<std::path::PathBuf>,
    /// Non-destructive audition buffer (Preview).
    preview: Option<AudioData>,
    /// Learned noise print for denoise / dialogue isolate.
    noise_print: Option<NoisePrint>,
    /// Amplitude histogram of the displayed buffer.
    histogram: LevelHistogram,
    /// Live peak meters at the playhead (linear 0…1) with ballistics.
    meter_l: f32,
    meter_r: f32,
    /// While dragging the scrubber, override displayed time (seconds) for instant UI.
    scrubbing: Option<f32>,
    /// When true, primary clock shows time remaining (to end); else time played.
    show_time_remaining: bool,
    /// OS-native File / Edit / View menu (macOS bar / Windows window menu).
    native_menu: Option<NativeMenu>,
    /// Spectrogram vs playlist central pane.
    viewer_mode: ViewerMode,
    /// Session media queue (paths). Current track is loaded via [`Self::open`].
    playlist: Vec<PlaylistEntry>,
    /// Highlighted row in the playlist viewer.
    playlist_sel: Option<usize>,
    /// When true, end-of-track advances to the next playlist item and plays.
    playlist_auto_advance: bool,
    /// Classic spectrum-bar visualizer state.
    visualizer: SpectrumViz,
    /// Debounce live EQ reloads while dragging faders.
    eq_needs_reload: bool,
}

impl CatharGui {
    /// Build the app; opens the audio device if one is available.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        let appearance = Appearance::Dark;
        theme::apply(&cc.egui_ctx, appearance);
        let resolved_theme = theme::resolved(&cc.egui_ctx, appearance);
        let logo = load_logo(&cc.egui_ctx, resolved_theme == Theme::Light);
        let engine = Engine::new().ok();
        let status = match &engine {
            Some(_) => "Open an audio file to begin (File → Open, or ⌘O / Ctrl+O).".to_string(),
            None => "No audio output device — editing works, playback is disabled.".to_string(),
        };
        let native_menu =
            NativeMenu::new().map_err(|e| eprintln!("native menu unavailable: {e}")).ok();
        Self {
            engine,
            original: None,
            history: Vec::new(),
            hist_idx: 0,
            texture: None,
            logo,
            spec: None,
            sample_rate: 0,
            duration: 0.0,
            n_channels: 0,
            waveform_l: Vec::new(),
            waveform_r: Vec::new(),
            channel_view: ChannelView::Mid,
            monitor: Monitor::Stereo,
            volume: 1.0,
            selection: None,
            drag_anchor: None,
            show_original: false,
            gain_db: -24.0,
            db_floor: -90.0,
            db_ceil: 0.0,
            zoom_x: 1.0,
            zoom_y: 1.0,
            fx: FxParams::default(),
            appearance,
            resolved_theme,
            status,
            active_module: None,
            module_filter: String::new(),
            file_name: None,
            file_path: None,
            preview: None,
            noise_print: None,
            histogram: LevelHistogram::default(),
            meter_l: 0.0,
            meter_r: 0.0,
            scrubbing: None,
            show_time_remaining: false,
            native_menu,
            viewer_mode: ViewerMode::Spectrogram,
            playlist: Vec::new(),
            playlist_sel: None,
            playlist_auto_advance: true,
            visualizer: SpectrumViz::new(),
            eq_needs_reload: false,
        }
    }

    fn sync_system_theme(&mut self, ctx: &egui::Context) {
        if self.appearance != Appearance::System {
            return;
        }
        let now = theme::resolved(ctx, Appearance::System);
        if now != self.resolved_theme {
            self.resolved_theme = now;
            theme::apply(ctx, Appearance::System);
            self.logo = load_logo(ctx, now == Theme::Light);
        }
    }

    fn apply_appearance(&mut self, ctx: &egui::Context, appearance: Appearance) {
        self.appearance = appearance;
        theme::apply(ctx, appearance);
        self.resolved_theme = theme::resolved(ctx, appearance);
        self.logo = load_logo(ctx, self.resolved_theme == Theme::Light);
    }

    fn has_audio(&self) -> bool {
        !self.history.is_empty()
    }

    fn displayed(&self) -> Option<&AudioData> {
        if self.show_original {
            return self.original.as_ref();
        }
        if let Some(p) = &self.preview {
            return Some(p);
        }
        self.history.get(self.hist_idx)
    }

    /// Buffer the next edit is computed from (history tip, not preview/original).
    fn source_edit(&self) -> Option<&AudioData> {
        self.history.get(self.hist_idx)
    }

    fn open(&mut self, ctx: &egui::Context, path: String) {
        match AudioData::from_file(&path) {
            Ok(audio) => {
                self.sample_rate = audio.sample_rate;
                self.n_channels = audio.channels.len();
                // Default display: split when stereo so L/R are both visible.
                self.channel_view =
                    if is_stereo(&audio) { ChannelView::Split } else { ChannelView::Mid };
                self.monitor = Monitor::Stereo;
                self.original = Some(audio.clone());
                self.history = vec![audio];
                self.hist_idx = 0;
                self.show_original = false;
                self.selection = None;
                self.preview = None;
                self.noise_print = None;
                let pbuf = std::path::PathBuf::from(&path);
                self.file_name = Some(
                    pbuf.file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.clone()),
                );
                self.file_path = Some(pbuf.clone());
                // Reflect in playlist selection if this path is queued.
                if let Some(i) = self.playlist.iter().position(|e| e.path == pbuf) {
                    self.playlist_sel = Some(i);
                } else if let Some(name) = self.file_name.clone() {
                    // Single open: ensure a one-track playlist entry for the toggle view.
                    if !self.playlist.iter().any(|e| e.path == pbuf) {
                        self.playlist.push(PlaylistEntry { path: pbuf, name });
                        self.playlist_sel = Some(self.playlist.len() - 1);
                    }
                }
                self.reload_engine(true);
                let ch = if self.n_channels >= 2 { "stereo" } else { "mono" };
                self.status = format!(
                    "Loaded {} ({ch}, {} Hz)",
                    self.file_name.as_deref().unwrap_or("file"),
                    self.sample_rate
                );
                self.sync_window_title(ctx);
                self.recompute(ctx);
            }
            Err(e) => self.status = format!("Failed to open: {e}"),
        }
    }

    /// Buffer fed to the player: current edit/preview, with live EQ if enabled.
    ///
    /// Live EQ is a **monitor-only** layer on top of [`Self::displayed`]. It must
    /// never be left active when the same curve is already baked into history/preview,
    /// or On/Off and Render will stack / fail to restore dry.
    fn audio_for_engine(&self) -> Option<AudioData> {
        let base = self.displayed()?.clone();
        if !self.fx.eq_enabled || eq_is_flat(&self.fx.eq_gains, self.fx.eq_preamp) {
            return Some(base);
        }
        let sr = base.sample_rate;
        let gains = self.fx.eq_gains;
        let pre = self.fx.eq_preamp;
        Some(base.map_channels(|c| apply_eq_gains(c, sr, &gains, pre)))
    }

    /// Push current buffer to the engine with monitor routing (volume is live).
    fn reload_engine(&mut self, reset_pos: bool) {
        let Some(audio) = self.audio_for_engine() else {
            return;
        };
        let Some(eng) = self.engine.as_mut() else {
            return;
        };
        eng.set_monitor(self.monitor);
        // Volume target only — load/reload keeps actual gain continuous.
        eng.set_volume(self.volume);
        if reset_pos {
            // Hard reset (open file, stop, etc.): always land paused at t=0.
            eng.pause();
            let _ = eng.load(&audio);
        } else {
            // Live EQ / monitor change: keep playhead + transport intent.
            let pos = eng.pos();
            let playing = eng.is_playing();
            let _ = eng.reload(&audio, pos, playing);
        }
        self.eq_needs_reload = false;
    }

    /// Mark EQ dirty — applied next frame so fader drags stay smooth.
    fn schedule_eq_reload(&mut self) {
        self.eq_needs_reload = true;
    }

    /// Apply live EQ on/off (or curve) to the player **now** — not deferred.
    /// Used for the On checkbox so Off always restores dry audio immediately.
    fn apply_eq_to_engine(&mut self) {
        if !self.has_audio() {
            self.eq_needs_reload = false;
            return;
        }
        self.reload_engine(false);
        self.status = if self.fx.eq_enabled && !eq_is_flat(&self.fx.eq_gains, self.fx.eq_preamp) {
            "EQ on — hearing current curve".into()
        } else if self.fx.eq_enabled {
            "EQ on (flat)".into()
        } else {
            "EQ off".into()
        };
    }

    fn seek_to(&mut self, t: f32) {
        let t = t.clamp(0.0, self.duration.max(0.0));
        if let Some(eng) = &self.engine {
            eng.seek(t);
        }
    }

    fn begin_scrub(&mut self) {
        if let Some(eng) = self.engine.as_mut() {
            eng.begin_scrub();
        }
    }

    fn end_scrub(&mut self) {
        if let Some(eng) = self.engine.as_mut() {
            eng.end_scrub();
        }
    }

    /// Map a pointer x (global screen) onto the scrub rail → seconds.
    fn time_from_scrub_x(rail: Rect, x: f32, duration: f32) -> f32 {
        if duration <= 0.0 || rail.width() <= 0.0 {
            return 0.0;
        }
        ((x - rail.left()) / rail.width()).clamp(0.0, 1.0) * duration
    }

    fn transport_play_pause(&mut self) {
        let need_reload = self.engine.as_ref().is_some_and(|e| e.needs_reload_to_restart());
        if need_reload {
            // Source fully finished — re-append buffer from t=0, then play.
            self.reload_engine(true);
            if let Some(eng) = self.engine.as_mut() {
                eng.play();
            }
            return;
        }
        if let Some(eng) = self.engine.as_mut() {
            eng.toggle();
        }
    }

    fn transport_stop(&mut self) {
        let at_end = self.engine.as_ref().is_some_and(|e| e.at_end());
        if at_end {
            // Finished source: reload at start instead of a dead seek.
            self.reload_engine(true);
        } else if let Some(eng) = self.engine.as_mut() {
            eng.stop();
        }
        self.meter_l = 0.0;
        self.meter_r = 0.0;
    }

    fn stereo_file(&self) -> bool {
        self.n_channels >= 2
    }

    fn save(&mut self, path: String) {
        let Some(audio) = self.history.get(self.hist_idx) else { return };
        match audio.to_file(&path) {
            Ok(()) => self.status = format!("Saved {path}"),
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn recompute(&mut self, ctx: &egui::Context) {
        let Some(audio) = self.displayed().cloned() else { return };
        let sr = audio.sample_rate;
        self.n_channels = audio.channels.len();
        self.histogram = LevelHistogram::compute(&audio.channels);
        let left = channel_samples(&audio, ChannelView::Left);
        let right = channel_samples(&audio, ChannelView::Right);
        let n = left.len().max(right.len());
        self.duration = if sr > 0 { n as f32 / sr as f32 } else { 0.0 };
        self.waveform_l = waveform_envelope(&left, 2000);
        self.waveform_r =
            if is_stereo(&audio) { waveform_envelope(&right, 2000) } else { Vec::new() };

        let hop = display_hop(n);
        match self.channel_view {
            ChannelView::Split if is_stereo(&audio) => {
                let sl = compute_spectrogram(&left, sr, FFT_SIZE, hop);
                let sr_ch = compute_spectrogram(&right, sr, FFT_SIZE, hop);
                // Cache L in `spec` for selection mapping; texture is stacked.
                let img_l = colorize(&sl, self.db_floor, self.db_ceil);
                let img_r = colorize(&sr_ch, self.db_floor, self.db_ceil);
                let stacked = stack_vertical(&img_l, &img_r);
                self.spec = Some(sl);
                self.texture =
                    Some(ctx.load_texture("spectrogram", stacked, TextureOptions::LINEAR));
            }
            view => {
                let samples = channel_samples(&audio, view);
                let hop = display_hop(samples.len());
                let spec = compute_spectrogram(&samples, sr, FFT_SIZE, hop);
                self.spec = Some(spec);
                self.recolor(ctx);
            }
        }
    }

    fn recolor(&mut self, ctx: &egui::Context) {
        // Split textures are rebuilt in `recompute` (two STFTs).
        if self.channel_view == ChannelView::Split && self.stereo_file() {
            if let Some(audio) = self.displayed().cloned() {
                let sr = audio.sample_rate;
                let left = channel_samples(&audio, ChannelView::Left);
                let right = channel_samples(&audio, ChannelView::Right);
                let hop = display_hop(left.len().max(right.len()));
                let sl = compute_spectrogram(&left, sr, FFT_SIZE, hop);
                let sr_ch = compute_spectrogram(&right, sr, FFT_SIZE, hop);
                let stacked = stack_vertical(
                    &colorize(&sl, self.db_floor, self.db_ceil),
                    &colorize(&sr_ch, self.db_floor, self.db_ceil),
                );
                self.spec = Some(sl);
                self.texture =
                    Some(ctx.load_texture("spectrogram", stacked, TextureOptions::LINEAR));
            }
            return;
        }
        let img = match &self.spec {
            Some(spec) => colorize(spec, self.db_floor, self.db_ceil),
            None => return,
        };
        self.texture = Some(ctx.load_texture("spectrogram", img, TextureOptions::LINEAR));
    }

    fn push_edit(&mut self, ctx: &egui::Context, audio: AudioData) {
        self.history.truncate(self.hist_idx + 1);
        self.history.push(audio);
        self.hist_idx = self.history.len() - 1;
        self.show_original = false;
        self.preview = None;
        self.reload_engine(true);
        self.recompute(ctx);
    }

    /// Commit `audio` as Render, or stage as Preview.
    fn finish_fx(&mut self, ctx: &egui::Context, audio: AudioData, label: &str, render: bool) {
        if render {
            self.push_edit(ctx, audio);
            self.status = format!("Rendered {label}");
        } else {
            self.preview = Some(audio);
            self.show_original = false;
            self.reload_engine(true);
            self.recompute(ctx);
            self.status = format!("Preview {label} — Render to commit");
        }
    }

    fn apply_whole<F: FnOnce(&AudioData) -> AudioData>(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        render: bool,
        f: F,
    ) {
        let Some(cur) = self.source_edit().cloned() else { return };
        let new = f(&cur);
        self.finish_fx(ctx, new, label, render);
    }

    fn clear_preview(&mut self, ctx: &egui::Context) {
        if self.preview.take().is_some() {
            self.reload_engine(true);
            self.recompute(ctx);
            self.status = "Bypass — preview cleared".into();
        }
    }

    fn toggle_compare(&mut self, ctx: &egui::Context) {
        if self.original.is_none() {
            return;
        }
        self.show_original = !self.show_original;
        if self.show_original {
            self.status = "Compare: original file".into();
        } else {
            self.status = "Compare: current edit".into();
        }
        self.reload_engine(true);
        self.recompute(ctx);
    }

    /// Preview / Bypass / Compare / Render with a snapshot of [`FxParams`].
    #[allow(clippy::too_many_arguments)]
    fn run_fx(
        &mut self,
        ctx: &egui::Context,
        preview: bool,
        bypass: bool,
        compare: bool,
        render: bool,
        label: &str,
        f: impl FnOnce(&AudioData, &FxParams) -> AudioData,
    ) {
        if bypass {
            self.clear_preview(ctx);
            return;
        }
        if compare {
            self.toggle_compare(ctx);
            return;
        }
        if preview || render {
            let Some(cur) = self.source_edit().cloned() else { return };
            let fx = self.fx.clone();
            let out = f(&cur, &fx);
            self.finish_fx(ctx, out, label, render);
        }
    }

    fn apply_selection(&mut self, ctx: &egui::Context, op: SpectralOp, label: &str, render: bool) {
        let (Some(sel), Some(cur)) = (self.selection, self.source_edit()) else {
            self.status = "Draw a selection on the spectrogram first.".into();
            return;
        };
        let sr = cur.sample_rate;
        let new = cur.map_channels(|c| apply_spectral(c, sr, &sel, op));
        self.finish_fx(ctx, new, label, render);
    }

    fn undo(&mut self, ctx: &egui::Context) {
        if self.hist_idx > 0 {
            self.hist_idx -= 1;
            self.show_original = false;
            self.reload_engine(true);
            self.recompute(ctx);
            self.status = "Undo".into();
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if self.hist_idx + 1 < self.history.len() {
            self.hist_idx += 1;
            self.show_original = false;
            self.reload_engine(true);
            self.recompute(ctx);
            self.status = "Redo".into();
        }
    }

    fn set_channel_view(&mut self, ctx: &egui::Context, view: ChannelView) {
        if view == ChannelView::Split && !self.stereo_file() {
            return;
        }
        if view == ChannelView::Right && !self.stereo_file() {
            return;
        }
        self.channel_view = view;
        self.recompute(ctx);
    }

    fn set_monitor(&mut self, m: Monitor) {
        self.monitor = m;
        self.reload_engine(false);
        self.status = match m {
            Monitor::Stereo => "Monitor: stereo (true L/R)".into(),
            Monitor::Left => "Monitor: left only".into(),
            Monitor::Right => "Monitor: right only".into(),
            Monitor::Mid => "Monitor: mid (mono sum)".into(),
        };
    }

    fn nyquist(&self) -> f32 {
        self.sample_rate as f32 / 2.0
    }
}

impl eframe::App for CatharGui {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if let Some(menu) = self.native_menu.as_mut() {
            menu.ensure_installed(frame);
        }
        self.handle_native_menu(ctx);
        self.sync_native_menu_enabled();
        self.handle_player_keys(ctx);

        // End-of-track: auto-advance playlist, else just pause.
        // Never seek-to-end here — rodio try_seek can hang once the source has
        // finished draining (was freezing the whole UI after a song ended).
        let at_end = self
            .engine
            .as_ref()
            .is_some_and(|eng| eng.is_loaded() && !eng.is_paused() && eng.at_end());
        if at_end && !(self.playlist_auto_advance && self.playlist_advance(ctx, 1, true)) {
            if let Some(eng) = self.engine.as_mut() {
                eng.pause();
            }
        }

        self.sync_system_theme(ctx);
        self.tick_meters();
        if self.viewer_mode == ViewerMode::Visualizer {
            self.tick_visualizer();
        }
        // Live EQ: rebuild monitor buffer when settings change and pointer is up
        // (avoids re-EQ'ing a long file on every fader pixel during a drag).
        let pointer_down = ctx.input(|i| i.pointer.any_down());
        if self.eq_needs_reload && self.has_audio() && !pointer_down {
            self.reload_engine(false);
            self.status = if self.fx.eq_enabled {
                "EQ on — hearing current curve".into()
            } else {
                "EQ off".into()
            };
        } else if self.eq_needs_reload {
            ctx.request_repaint();
        }
        // Dezipper volume every frame so slider moves don't click.
        let volume_ramping = self.engine.as_mut().is_some_and(|e| e.tick_volume());

        // No in-window File/Edit/View — those live in the OS menu bar.
        self.toolbar(ctx);
        if self.viewer_mode == ViewerMode::Spectrogram {
            self.overview_strip(ctx);
        }
        self.player_bar(ctx);
        self.modules_panel(ctx);
        self.central(ctx);
        self.module_window(ctx);

        // ~60 fps while playing, visualizer open, or volume still ramping.
        let viz_live = self.viewer_mode == ViewerMode::Visualizer && self.has_audio();
        if let Some(eng) = &self.engine {
            if eng.is_playing() || volume_ramping || viz_live {
                ctx.request_repaint();
            } else if self.has_audio() {
                ctx.request_repaint_after(std::time::Duration::from_millis(33));
            }
        } else if viz_live {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
}

impl CatharGui {
    fn sync_window_title(&self, ctx: &egui::Context) {
        // Title bar: track name only (product name lives in the menu bar / Dock).
        let title = self.file_name.clone().unwrap_or_else(|| "Cathar".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }

    fn sync_native_menu_enabled(&self) {
        let Some(menu) = self.native_menu.as_ref() else {
            return;
        };
        menu.set_enabled(
            self.has_audio(),
            self.hist_idx > 0,
            self.hist_idx + 1 < self.history.len(),
        );
    }

    fn handle_native_menu(&mut self, ctx: &egui::Context) {
        for action in native_menu::poll_events() {
            match action.as_str() {
                native_menu::id::OPEN => self.pick_open(ctx),
                native_menu::id::OPEN_PLAYLIST => {
                    self.pick_add_playlist(ctx);
                    self.viewer_mode = ViewerMode::Playlist;
                }
                native_menu::id::IMPORT_M3U => self.pick_import_m3u(ctx),
                native_menu::id::SAVE => self.pick_save(),
                native_menu::id::UNDO => self.undo(ctx),
                native_menu::id::REDO => self.redo(ctx),
                native_menu::id::VIEW_SPECTRO => {
                    self.viewer_mode = ViewerMode::Spectrogram;
                }
                native_menu::id::VIEW_PLAYLIST => {
                    self.viewer_mode = ViewerMode::Playlist;
                }
                native_menu::id::VIEW_VIZ => {
                    self.viewer_mode = ViewerMode::Visualizer;
                }
                native_menu::id::OPEN_EQ => {
                    self.active_module = Some(Module::Equalizer);
                }
                native_menu::id::THEME_SYSTEM => self.apply_appearance(ctx, Appearance::System),
                native_menu::id::THEME_LIGHT => self.apply_appearance(ctx, Appearance::Light),
                native_menu::id::THEME_DARK => self.apply_appearance(ctx, Appearance::Dark),
                native_menu::id::RESET_ZOOM => {
                    self.zoom_x = 1.0;
                    self.zoom_y = 1.0;
                }
                _ => {}
            }
        }
    }

    fn pick_open(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif"])
            .pick_file()
        {
            self.open(ctx, p.display().to_string());
        }
    }

    /// Multi-select audio files and append to the session playlist.
    fn pick_add_playlist(&mut self, ctx: &egui::Context) {
        let files = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif"])
            .pick_files();
        let Some(files) = files else { return };
        let mut added = 0usize;
        let load_first = self.playlist.is_empty() && !self.has_audio();
        for p in files {
            let name = p
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.display().to_string());
            let path = p.clone();
            if self.playlist.iter().any(|e| e.path == path) {
                continue;
            }
            self.playlist.push(PlaylistEntry { path, name });
            added += 1;
        }
        if added == 0 {
            self.status = "No new tracks added to playlist".into();
            return;
        }
        self.status = format!("Added {added} track(s) to playlist");
        if load_first {
            if let Some(first) = self.playlist.first().map(|e| e.path.display().to_string()) {
                self.playlist_sel = Some(0);
                self.open(ctx, first);
            }
        }
    }

    fn load_playlist_index(&mut self, ctx: &egui::Context, idx: usize) {
        self.load_playlist_index_play(ctx, idx, false);
    }

    fn load_playlist_index_play(&mut self, ctx: &egui::Context, idx: usize, autoplay: bool) {
        let Some(entry) = self.playlist.get(idx).cloned() else {
            return;
        };
        self.playlist_sel = Some(idx);
        self.open(ctx, entry.path.display().to_string());
        self.viewer_mode = ViewerMode::Spectrogram;
        if autoplay {
            if let Some(eng) = self.engine.as_mut() {
                eng.play();
            }
            self.status = format!("Playing · {}", entry.name);
        } else {
            self.status = format!("Loaded · {}", entry.name);
        }
    }

    /// Step playlist by `delta` (−1 / +1). Returns true if a new track was loaded.
    fn playlist_advance(&mut self, ctx: &egui::Context, delta: i32, autoplay: bool) -> bool {
        if self.playlist.is_empty() {
            return false;
        }
        let n = self.playlist.len() as i32;
        let cur = self
            .playlist_sel
            .or_else(|| {
                self.file_path
                    .as_ref()
                    .and_then(|p| self.playlist.iter().position(|e| &e.path == p))
            })
            .unwrap_or(0) as i32;
        let next = cur + delta;
        if next < 0 || next >= n {
            return false;
        }
        self.load_playlist_index_play(ctx, next as usize, autoplay);
        true
    }

    fn playlist_prev(&mut self, ctx: &egui::Context) {
        // iTunes-style: if >2s into the track, restart; else previous.
        let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
        if pos > 2.0 {
            self.seek_to(0.0);
            self.status = "Restart track".into();
            return;
        }
        if !self.playlist_advance(ctx, -1, true) {
            self.seek_to(0.0);
            self.status = "Start of playlist".into();
        }
    }

    fn playlist_next(&mut self, ctx: &egui::Context) {
        if !self.playlist_advance(ctx, 1, true) {
            self.status = "End of playlist".into();
            if let Some(eng) = self.engine.as_mut() {
                eng.pause();
            }
        }
    }

    fn pick_import_m3u(&mut self, ctx: &egui::Context) {
        let Some(path) =
            rfd::FileDialog::new().add_filter("Playlist", &["m3u", "m3u8"]).pick_file()
        else {
            return;
        };
        match parse_m3u(&path) {
            Ok(paths) if paths.is_empty() => {
                self.status = "M3U had no local audio files".into();
            }
            Ok(paths) => {
                let load_first = self.playlist.is_empty() && !self.has_audio();
                let mut added = 0usize;
                for p in paths {
                    if self.playlist.iter().any(|e| e.path == p) {
                        continue;
                    }
                    let name = p
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string());
                    self.playlist.push(PlaylistEntry { path: p, name });
                    added += 1;
                }
                self.viewer_mode = ViewerMode::Playlist;
                self.status = format!("Imported {added} track(s) from M3U");
                if load_first && !self.playlist.is_empty() {
                    self.load_playlist_index_play(ctx, 0, false);
                }
            }
            Err(e) => self.status = format!("M3U import failed: {e}"),
        }
    }

    fn pick_save(&mut self) {
        if !self.has_audio() {
            return;
        }
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Audio", &["wav", "flac", "aiff"])
            .set_file_name("edited.wav")
            .save_file()
        {
            self.save(p.display().to_string());
        }
    }

    /// Transport + file + View shortcuts — ignored while typing in a text field.
    ///
    /// Mirrors OS-menu accelerators so Linux (and menu-init lag) still work.
    fn handle_player_keys(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        if typing {
            return;
        }

        let keys = ctx.input(|i| {
            let cmd = i.modifiers.command;
            let alt = i.modifiers.alt;
            let shift = i.modifiers.shift;
            (
                cmd && i.key_pressed(egui::Key::O) && !shift,
                cmd && shift && i.key_pressed(egui::Key::O),
                cmd && i.key_pressed(egui::Key::S),
                cmd && i.key_pressed(egui::Key::E),
                cmd && !alt && i.key_pressed(egui::Key::Num1),
                cmd && !alt && i.key_pressed(egui::Key::Num2),
                cmd && !alt && i.key_pressed(egui::Key::Num3),
                cmd && alt && i.key_pressed(egui::Key::Num1),
                cmd && alt && i.key_pressed(egui::Key::Num2),
                cmd && alt && i.key_pressed(egui::Key::Num3),
                cmd && i.key_pressed(egui::Key::Num0),
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Home),
                i.key_pressed(egui::Key::Z),
                i.key_pressed(egui::Key::Y),
            )
        });
        let (
            cmd_o,
            cmd_shift_o,
            cmd_s,
            cmd_e,
            cmd_1,
            cmd_2,
            cmd_3,
            cmd_alt_1,
            cmd_alt_2,
            cmd_alt_3,
            cmd_0,
            space,
            left,
            right,
            home,
            z,
            y,
        ) = keys;

        if cmd_o {
            self.pick_open(ctx);
            return;
        }
        if cmd_shift_o {
            self.pick_add_playlist(ctx);
            self.viewer_mode = ViewerMode::Playlist;
            return;
        }
        if cmd_s {
            self.pick_save();
            return;
        }
        if cmd_e {
            self.active_module = Some(Module::Equalizer);
            return;
        }
        if cmd_1 {
            self.viewer_mode = ViewerMode::Spectrogram;
            return;
        }
        if cmd_2 {
            self.viewer_mode = ViewerMode::Playlist;
            return;
        }
        if cmd_3 {
            self.viewer_mode = ViewerMode::Visualizer;
            return;
        }
        if cmd_alt_1 {
            self.apply_appearance(ctx, Appearance::System);
            return;
        }
        if cmd_alt_2 {
            self.apply_appearance(ctx, Appearance::Light);
            return;
        }
        if cmd_alt_3 {
            self.apply_appearance(ctx, Appearance::Dark);
            return;
        }
        if cmd_0 {
            self.zoom_x = 1.0;
            self.zoom_y = 1.0;
            return;
        }

        if space && self.has_audio() {
            self.transport_play_pause();
        }
        if left && self.has_audio() {
            if let Some(eng) = &self.engine {
                eng.skip(-5.0);
            }
        }
        if right && self.has_audio() {
            if let Some(eng) = &self.engine {
                eng.skip(5.0);
            }
        }
        if home && self.has_audio() {
            self.seek_to(0.0);
        }
        if z {
            self.undo(ctx);
        }
        if y {
            self.redo(ctx);
        }
    }
}

impl CatharGui {
    /// Spectrogram channel (Display) + history/A-B.
    /// File name lives only in the window title. Listen lives on the player bar.
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar")
            .exact_height(44.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::well_bg())
                    .inner_margin(egui::Margin::symmetric(12.0, 6.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Single row: Display takes natural width on the left; history
                // docks right via remaining space (no overlapping allocate regions).
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    // Viewer modes — icons only (labels on tooltip), player-style chrome.
                    for (mode, icon, tip) in [
                        (ViewerMode::Spectrogram, icons::WAVEFORM, "Spectrogram"),
                        (ViewerMode::Visualizer, icons::CHART_BAR, "Visualizer"),
                        (ViewerMode::Playlist, icons::PLAYLIST, "Playlist"),
                    ] {
                        let sel = self.viewer_mode == mode;
                        if ui.add(toolbar_toggle(sel, icon)).on_hover_text(tip).clicked() {
                            self.viewer_mode = mode;
                        }
                    }

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);

                    ui.label(rich(icons::MONITOR, 15.0).color(theme::text_muted()))
                        .on_hover_text("Display — spectrogram channel (what you see / edit)");
                    let stereo = self.stereo_file();
                    let display_en =
                        self.has_audio() && self.viewer_mode == ViewerMode::Spectrogram;
                    for (view, enabled) in [
                        (ChannelView::Left, true),
                        (ChannelView::Right, stereo),
                        (ChannelView::Mid, true),
                        (ChannelView::Split, stereo),
                    ] {
                        let sel = self.channel_view == view;
                        if ui
                            .add_enabled(enabled && display_en, channel_chip(sel, view.label()))
                            .on_hover_text(match view {
                                ChannelView::Left => "Spectrogram: left channel",
                                ChannelView::Right => "Spectrogram: right channel",
                                ChannelView::Mid => "Spectrogram: mid (L+R)/2",
                                ChannelView::Split => "Stacked L (top) + R (bottom)",
                            })
                            .clicked()
                        {
                            self.set_channel_view(ctx, view);
                        }
                    }

                    // Remaining width → pack A/B · Undo · Redo on the far right.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ui
                            .add_enabled(
                                self.hist_idx + 1 < self.history.len(),
                                toolbar_button(icons::ARROWS_CLOCKWISE),
                            )
                            .on_hover_text("Redo (⇧⌘Z / Y)")
                            .clicked()
                        {
                            self.redo(ctx);
                        }
                        if ui
                            .add_enabled(
                                self.hist_idx > 0,
                                toolbar_button(icons::ARROW_COUNTER_CLOCKWISE),
                            )
                            .on_hover_text("Undo (⌘Z / Z)")
                            .clicked()
                        {
                            self.undo(ctx);
                        }
                        let ab_enabled = self.original.is_some();
                        let mut ab = self.show_original;
                        if ui
                            .add_enabled(ab_enabled, toolbar_toggle(ab, icons::SWAP))
                            .on_hover_text("Compare original (A/B)")
                            .clicked()
                        {
                            ab = !ab;
                            self.show_original = ab;
                            self.reload_engine(true);
                            self.recompute(ctx);
                        }
                    });
                });
            });
    }

    /// Overview waveform across the top — dual strip when stereo. Click/drag to scrub.
    fn overview_strip(&mut self, ctx: &egui::Context) {
        let h = if self.stereo_file() && !self.waveform_r.is_empty() { 72.0 } else { 56.0 };
        egui::TopBottomPanel::top("overview")
            .exact_height(h)
            .frame(
                egui::Frame::none()
                    .fill(theme::well_bg())
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0)),
            )
            .show(ctx, |ui| {
                let (resp, painter) =
                    ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
                let rect = resp.rect;
                painter.rect_filled(rect, 0.0, theme::well_bg());

                // Played region fill (progress under the overview).
                let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
                if self.duration > 0.0 {
                    let px = rect.left() + (pos / self.duration).clamp(0.0, 1.0) * rect.width();
                    painter.rect_filled(
                        Rect::from_min_max(rect.min, pos2(px, rect.bottom())),
                        0.0,
                        theme::selection_fill(),
                    );
                }

                if self.stereo_file() && !self.waveform_r.is_empty() {
                    let mid_y = rect.center().y;
                    let top = Rect::from_min_max(rect.min, pos2(rect.right(), mid_y - 1.0));
                    let bot = Rect::from_min_max(pos2(rect.left(), mid_y + 1.0), rect.max);
                    painter.line_segment(
                        [pos2(rect.left(), mid_y), pos2(rect.right(), mid_y)],
                        Stroke::new(1.0, theme::hairline()),
                    );
                    self.draw_waveform_env(&painter, top, &self.waveform_l, theme::wave_l());
                    self.draw_waveform_env(&painter, bot, &self.waveform_r, theme::wave_r());
                    painter.text(
                        pos2(rect.left() + 4.0, top.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        "L",
                        egui::FontId::proportional(10.0),
                        theme::wave_l(),
                    );
                    painter.text(
                        pos2(rect.left() + 4.0, bot.top() + 2.0),
                        egui::Align2::LEFT_TOP,
                        "R",
                        egui::FontId::proportional(10.0),
                        theme::wave_r(),
                    );
                } else {
                    self.draw_waveform_env(&painter, rect, &self.waveform_l, theme::wave_tint());
                }

                self.draw_playhead(&painter, rect);
                if let Some(sel) = self.selection {
                    if self.duration > 0.0 {
                        let x0 =
                            rect.left() + (sel.t0 / self.duration).clamp(0.0, 1.0) * rect.width();
                        let x1 =
                            rect.left() + (sel.t1 / self.duration).clamp(0.0, 1.0) * rect.width();
                        painter.rect_filled(
                            Rect::from_min_max(pos2(x0, rect.top()), pos2(x1, rect.bottom())),
                            0.0,
                            theme::selection_fill(),
                        );
                    }
                }

                // Overview scrub: UI-only while dragging; one seek on release.
                if self.has_audio() && self.duration > 0.0 {
                    if resp.drag_started() {
                        self.begin_scrub();
                    }
                    let dragging = resp.dragged() || resp.drag_started();
                    if dragging {
                        if let Some(x) = ctx
                            .pointer_interact_pos()
                            .or_else(|| resp.interact_pointer_pos())
                            .map(|p| p.x)
                        {
                            // Visual only — no engine seeks mid-drag.
                            self.scrubbing = Some(Self::time_from_scrub_x(rect, x, self.duration));
                            ctx.request_repaint();
                        }
                    } else if resp.clicked() {
                        if let Some(x) = ctx
                            .pointer_interact_pos()
                            .or_else(|| resp.interact_pointer_pos())
                            .map(|p| p.x)
                        {
                            let t = Self::time_from_scrub_x(rect, x, self.duration);
                            self.begin_scrub();
                            if let Some(eng) = &self.engine {
                                eng.seek_scrub(t);
                            }
                            self.end_scrub();
                        }
                    }
                    if resp.drag_stopped() {
                        if let Some(t) = self.scrubbing.take() {
                            if let Some(eng) = &self.engine {
                                eng.seek_scrub(t);
                            }
                        }
                        self.end_scrub();
                    }
                }
                resp.on_hover_cursor(egui::CursorIcon::PointingHand)
                    .on_hover_text("Click or drag to seek");
            });
    }

    /// Update L/R meters from samples under the playhead (not whole-file peaks).
    fn tick_meters(&mut self) {
        let playing = self.engine.as_ref().map(|e| e.is_playing()).unwrap_or(false);
        let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
        let sr = self.sample_rate;
        if sr == 0 {
            self.meter_l = 0.0;
            self.meter_r = 0.0;
            return;
        }

        // ~12 ms analysis window around the playhead.
        const WIN_MS: f32 = 12.0;
        let half = ((WIN_MS * 0.001 * sr as f32) * 0.5).round() as usize;
        let half = half.max(32);

        // Snapshot instants before mutating meter state (avoids borrow clash).
        let (instant_l, instant_r) = match self.displayed() {
            None => (0.0, 0.0),
            Some(audio) => {
                let l = live_peak(
                    audio.channels.first().map(Vec::as_slice).unwrap_or(&[]),
                    pos,
                    sr,
                    half,
                );
                let r = if audio.channels.len() >= 2 {
                    live_peak(&audio.channels[1], pos, sr, half)
                } else {
                    0.0 // mono: only L meter is drawn
                };
                (l, r)
            }
        };

        // Ballistics: instant attack, slower release while playing.
        let release = if playing { 0.88 } else { 0.75 };
        self.meter_l = if instant_l >= self.meter_l { instant_l } else { self.meter_l * release };
        self.meter_r = if instant_r >= self.meter_r { instant_r } else { self.meter_r * release };
        if self.meter_l < 1e-4 {
            self.meter_l = 0.0;
        }
        if self.meter_r < 1e-4 {
            self.meter_r = 0.0;
        }
    }

    /// Bottom transport strip.
    ///
    /// Layout is intentionally dumb and fixed so it cannot reflow:
    /// `[transport][time][scrub grows][🎧 chips][meters][volume]`
    /// All LTR, all fixed widths except scrub (min/max clamped).
    fn player_bar(&mut self, ctx: &egui::Context) {
        // Content ≈ 36px + 6+6 margin = 48; use 52 for hairline breathing room.
        egui::TopBottomPanel::bottom("player")
            .exact_height(52.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::player_bar())
                    .stroke(Stroke::new(1.0, theme::hairline()))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0)),
            )
            .show(ctx, |ui| {
                let eng_pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
                let pos = self.scrubbing.unwrap_or(eng_pos);
                let dur = self.duration.max(0.0);
                let playing = self.engine.as_ref().map(|e| e.is_playing()).unwrap_or(false);
                let has = self.has_audio();
                let stereo = self.stereo_file();
                let has_queue = self.playlist.len() > 1;

                // Plain horizontal — not horizontal_centered (can flip RTL on some systems).
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.spacing_mut().item_spacing.y = 0.0;

                    // ── Transport ──────────────────────────────────────────
                    if ui
                        .add_enabled(has || has_queue, toolbar_button(icons::SKIP_BACK))
                        .on_hover_text(if has_queue { "Previous track" } else { "Start (Home)" })
                        .clicked()
                    {
                        self.scrubbing = None;
                        if has_queue {
                            self.playlist_prev(ctx);
                        } else {
                            self.seek_to(0.0);
                        }
                    }
                    if ui
                        .add_enabled(has, toolbar_button(icons::REWIND))
                        .on_hover_text("Back 5s")
                        .clicked()
                    {
                        if let Some(eng) = &self.engine {
                            eng.skip(-5.0);
                        }
                    }
                    let play_icon = if playing { icons::PAUSE } else { icons::PLAY };
                    if ui
                        .add_enabled(has, transport_play_button(playing, play_icon))
                        .on_hover_text(if playing { "Pause" } else { "Play" })
                        .clicked()
                    {
                        self.transport_play_pause();
                    }
                    if ui
                        .add_enabled(has, toolbar_button(icons::STOP))
                        .on_hover_text("Stop")
                        .clicked()
                    {
                        self.transport_stop();
                    }
                    if ui
                        .add_enabled(has, toolbar_button(icons::FAST_FORWARD))
                        .on_hover_text("Forward 5s")
                        .clicked()
                    {
                        if let Some(eng) = &self.engine {
                            eng.skip(5.0);
                        }
                    }
                    if ui
                        .add_enabled(has_queue, toolbar_button(icons::SKIP_FORWARD))
                        .on_hover_text("Next track")
                        .clicked()
                    {
                        self.playlist_next(ctx);
                    }

                    ui.separator();

                    // ── Time: "MM:SS.s / MM:SS.s" fixed slot ───────────────
                    let remaining = (dur - pos).max(0.0);
                    let t_left = if self.show_time_remaining {
                        format!("−{}", fmt_time_player(remaining))
                    } else {
                        fmt_time_player(pos)
                    };
                    let t_right = fmt_time_player(dur);
                    let time_tip = if self.show_time_remaining {
                        "Remaining — click for elapsed"
                    } else {
                        "Elapsed — click for remaining"
                    };
                    let clock = format!("{t_left}  /  {t_right}");
                    let time_r = ui
                        .add_sized(
                            [108.0, 32.0],
                            egui::Label::new(
                                egui::RichText::new(clock)
                                    .monospace()
                                    .size(12.5)
                                    .color(theme::text()),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(time_tip);
                    if time_r.clicked() {
                        self.show_time_remaining = !self.show_time_remaining;
                    }

                    ui.separator();

                    // ── Scrub: takes leftover after fixed right block ───────
                    // Right block ≈ headphones+chips(190) + meters(120) + vol(140) + gaps
                    const RIGHT_FIXED: f32 = 190.0 + 120.0 + 140.0 + 24.0;
                    let scrub_w = (ui.available_width() - RIGHT_FIXED).clamp(64.0, 360.0);
                    self.player_scrubber(ui, scrub_w, pos, dur, has);

                    ui.separator();

                    // ── Listen (headphones) + routing chips ──
                    ui.label(rich(icons::HEADPHONES, 15.0).color(theme::text_muted()))
                        .on_hover_text("Listen — what you hear (independent of Display)");
                    for (m, label) in [
                        (Monitor::Stereo, "LR"),
                        (Monitor::Left, "L"),
                        (Monitor::Right, "R"),
                        (Monitor::Mid, "M"),
                    ] {
                        let sel = self.monitor == m;
                        let en = has && (m != Monitor::Right || stereo);
                        if ui
                            .add_enabled(en, channel_chip(sel, label))
                            .on_hover_text(match m {
                                Monitor::Stereo => "Listen: stereo",
                                Monitor::Left => "Listen: left only",
                                Monitor::Right => "Listen: right only",
                                Monitor::Mid => "Listen: mid mono",
                            })
                            .clicked()
                        {
                            self.set_monitor(m);
                        }
                    }

                    ui.separator();

                    // ── Meters (fixed dB width inside vu_meter_h) ──────────
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        vu_meter_h(ui, "L", self.meter_l, 72.0);
                        vu_meter_h(ui, "R", self.meter_r, 72.0);
                    });

                    ui.separator();

                    // ── Volume ─────────────────────────────────────────────
                    self.player_volume_control(ui);
                });
            });
    }

    fn player_scrubber(&mut self, ui: &mut egui::Ui, scrub_w: f32, pos: f32, dur: f32, has: bool) {
        let scrub_size = egui::vec2(scrub_w, 32.0);
        let (scrub_rect, _) = ui.allocate_exact_size(scrub_size, Sense::hover());
        let scrub_id = ui.make_persistent_id("player_scrubber");
        let scrub_resp = ui.interact(scrub_rect, scrub_id, Sense::click_and_drag());

        const RAIL_H: f32 = 4.0;
        let rail = Rect::from_center_size(
            scrub_rect.center(),
            egui::vec2(scrub_rect.width().max(1.0), RAIL_H),
        );
        ui.painter().rect_filled(rail, 2.0, theme::well_bg());
        ui.painter().rect_stroke(rail, 2.0, Stroke::new(1.0, theme::hairline()));

        let frac = if dur > 0.0 { (pos / dur).clamp(0.0, 1.0) } else { 0.0 };
        let filled_w = (rail.width() * frac).max(if frac > 0.0 { 2.0 } else { 0.0 });
        if filled_w > 0.0 {
            ui.painter().rect_filled(
                Rect::from_min_size(rail.min, egui::vec2(filled_w, rail.height())),
                2.0,
                theme::accent(),
            );
        }
        let thumb_x = (rail.left() + filled_w).clamp(rail.left() + 5.0, rail.right() - 5.0);
        let thumb = pos2(thumb_x, scrub_rect.center().y);
        const THUMB_R: f32 = 7.0;
        ui.painter().circle_filled(thumb, THUMB_R, theme::surface());
        ui.painter().circle_stroke(thumb, THUMB_R, Stroke::new(1.5, theme::accent()));

        if has && dur > 0.0 {
            if scrub_resp.drag_started() {
                // Hard-mute for the whole drag (volume 0 + pause).
                self.begin_scrub();
            }
            let dragging = scrub_resp.dragged() || scrub_resp.drag_started();
            if dragging {
                if let Some(p) =
                    ui.ctx().pointer_interact_pos().or_else(|| scrub_resp.interact_pointer_pos())
                {
                    // UI playhead only — never seek the engine mid-drag.
                    self.scrubbing = Some(Self::time_from_scrub_x(rail, p.x, dur));
                    ui.ctx().request_repaint();
                }
            } else if scrub_resp.clicked() {
                // Click while playing: mute → seek → fade back (same as drag end).
                if let Some(p) =
                    ui.ctx().pointer_interact_pos().or_else(|| scrub_resp.interact_pointer_pos())
                {
                    let t = Self::time_from_scrub_x(rail, p.x, dur);
                    self.begin_scrub();
                    if let Some(eng) = &self.engine {
                        eng.seek_scrub(t);
                    }
                    self.end_scrub();
                }
            }
            if scrub_resp.drag_stopped() {
                // One seek at release (still at volume 0), then fade in.
                if let Some(t) = self.scrubbing.take() {
                    if let Some(eng) = &self.engine {
                        eng.seek_scrub(t);
                    }
                }
                self.end_scrub();
            }
        }
        scrub_resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Seek");
    }

    fn player_volume_control(&mut self, ui: &mut egui::Ui) {
        const RAIL_W: f32 = 80.0;
        const RAIL_H: f32 = 5.0;
        const THUMB_R: f32 = 6.5;
        const VOL_MAX: f32 = 1.5;

        ui.spacing_mut().item_spacing.x = 5.0;
        ui.label(rich(icons::SPEAKER_HIGH, 14.0).color(theme::text_muted()));

        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(RAIL_W + THUMB_R * 2.0, 28.0), Sense::hover());
        let resp =
            ui.interact(rect, ui.make_persistent_id("player_volume"), Sense::click_and_drag());

        let rail = Rect::from_center_size(rect.center(), egui::vec2(RAIL_W, RAIL_H));
        ui.painter().rect_filled(rail, 2.0, theme::well_bg());
        ui.painter().rect_stroke(rail, 2.0, Stroke::new(1.0, theme::hairline()));

        let t = (self.volume / VOL_MAX).clamp(0.0, 1.0);
        let filled_w = (rail.width() * t).max(if t > 0.0 { 2.0 } else { 0.0 });
        if filled_w > 0.0 {
            ui.painter().rect_filled(
                Rect::from_min_size(rail.min, egui::vec2(filled_w, rail.height())),
                2.0,
                theme::accent(),
            );
        }
        let thumb_x = (rail.left() + filled_w).clamp(rail.left() + 4.0, rail.right() - 4.0);
        let thumb = pos2(thumb_x, rail.center().y);
        ui.painter().circle_filled(thumb, THUMB_R, theme::surface());
        ui.painter().circle_stroke(thumb, THUMB_R, Stroke::new(1.5, theme::accent()));

        if resp.dragged() || resp.clicked() {
            if let Some(p) = ui.ctx().pointer_interact_pos().or_else(|| resp.interact_pointer_pos())
            {
                let u = ((p.x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
                self.volume = u * VOL_MAX;
                if let Some(eng) = self.engine.as_mut() {
                    eng.set_volume(self.volume);
                }
                ui.ctx().request_repaint();
            }
        }
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Volume");

        ui.add_sized(
            [34.0, 14.0],
            egui::Label::new(
                egui::RichText::new(format!("{:>3.0}%", self.volume * 100.0))
                    .monospace()
                    .size(11.0)
                    .color(theme::text_muted()),
            ),
        );
    }

    fn modules_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("modules")
            .default_width(panel::MODULES_W)
            .width_range(240.0..=340.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(theme::chrome_bg())
                    .inner_margin(egui::Margin::symmetric(10.0, 10.0)),
            )
            .show(ctx, |ui| {
                // Header
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(icons::SPARKLE)
                            .family(icons::family())
                            .size(16.0)
                            .color(theme::accent()),
                    );
                    ui.label(
                        egui::RichText::new("Toolbox")
                            .size(theme::FONT_TITLE)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(m) = self.active_module {
                            ui.label(
                                egui::RichText::new(m.title())
                                    .size(theme::FONT_CAPTION)
                                    .color(theme::accent()),
                            );
                        }
                    });
                });
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("Grouped by what you fix — pick a tool, Preview, Render.")
                        .size(theme::FONT_CAPTION)
                        .color(theme::text_muted()),
                );
                ui.add_space(10.0);

                // Search — shared card chrome (matches Levels wells / module cards).
                theme::card_frame().show(ui, |ui| {
                    ui.set_min_height(18.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.module_filter)
                            .hint_text("Search tools…")
                            .desired_width(ui.available_width())
                            .frame(false)
                            .font(egui::FontId::proportional(theme::FONT_BODY)),
                    );
                });
                ui.add_space(10.0);

                let enabled = self.has_audio();
                let q = self.module_filter.to_ascii_lowercase();
                let filtering = !q.is_empty();
                let mut open: Option<Module> = None;

                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    if filtering {
                        // Flat filtered results — ignore collapse when searching.
                        ui.label(
                            egui::RichText::new("Results").size(11.0).color(theme::text_muted()),
                        );
                        ui.add_space(4.0);
                        let mut any = false;
                        for g in TOOL_GROUPS {
                            for &m in g.tools {
                                if !m.matches_filter(&q) {
                                    continue;
                                }
                                any = true;
                                let sel = self.active_module == Some(m);
                                if tool_tile(ui, m.icon(), m.title(), m.blurb(), sel, enabled) {
                                    open = Some(m);
                                }
                                ui.add_space(3.0);
                            }
                        }
                        if !any {
                            hint(ui, "No tools match that search.");
                        }
                    } else {
                        for g in TOOL_GROUPS {
                            // Force group open if active tool is inside it.
                            let contains_active =
                                self.active_module.is_some_and(|a| g.tools.contains(&a));
                            tool_group(
                                ui,
                                g.id,
                                g.icon,
                                g.title,
                                g.subtitle,
                                g.accent,
                                g.default_open,
                                contains_active,
                                |ui| {
                                    for &m in g.tools {
                                        let sel = self.active_module == Some(m);
                                        if tool_tile(
                                            ui,
                                            m.icon(),
                                            m.title(),
                                            m.blurb(),
                                            sel,
                                            enabled,
                                        ) {
                                            open = Some(m);
                                        }
                                        ui.add_space(3.0);
                                    }
                                },
                            );
                        }
                    }

                    // ── Levels / Spectrogram / History (shared section style) ──
                    side_section(ui, "Levels");
                    if self.has_audio() {
                        self.histogram.show(ui, 110.0);
                    } else {
                        hint(ui, "Open audio to see the level histogram.");
                    }

                    side_section(ui, "Spectrogram");
                    let r1 = param_f32(ui, "dB floor", &mut self.db_floor, -120.0..=-30.0, 0);
                    let r2 = param_f32(ui, "dB ceiling", &mut self.db_ceil, -30.0..=6.0, 0);
                    if r1.changed() || r2.changed() {
                        self.recolor(ctx);
                    }
                    param_f32(ui, "Zoom time", &mut self.zoom_x, 1.0..=12.0, 1);
                    param_f32(ui, "Zoom freq", &mut self.zoom_y, 1.0..=8.0, 1);

                    side_section(ui, "History");
                    if self.history.is_empty() {
                        hint(ui, "Edits appear here after Render.");
                    } else {
                        for i in 0..self.history.len() {
                            let label = if i == 0 {
                                "Initial State".to_string()
                            } else {
                                format!("Edit {i}")
                            };
                            let selected = i == self.hist_idx && self.preview.is_none();
                            if compact_row(ui, &label, selected, true) {
                                self.hist_idx = i;
                                self.show_original = false;
                                self.preview = None;
                                self.reload_engine(true);
                                self.recompute(ctx);
                                self.status = format!("Restored history #{i}");
                            }
                        }
                        if self.preview.is_some() {
                            ui.label(
                                egui::RichText::new("● Preview (uncommitted)")
                                    .size(theme::FONT_CAPTION)
                                    .color(theme::warn()),
                            );
                        }
                    }

                    if !enabled {
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("Open a file to unlock tools.")
                                .size(theme::FONT_LABEL)
                                .color(theme::text_muted())
                                .italics(),
                        );
                    }
                });

                if let Some(m) = open {
                    self.active_module = Some(m);
                }
            });
    }

    fn module_window(&mut self, ctx: &egui::Context) {
        let Some(module) = self.active_module else { return };
        let mut open = true;
        let title = if self.preview.is_some() {
            format!("{}  ·  preview", module.title())
        } else {
            module.title().to_string()
        };
        egui::Window::new(title)
            .id(egui::Id::new("active_module"))
            .open(&mut open)
            .resizable(true)
            .collapsible(false)
            .default_pos(pos2(80.0, 120.0))
            .default_width(panel::MODULE_WIN_W)
            .default_height(520.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(theme::window_bg())
                    .stroke(theme::stroke_hairline())
                    .rounding(theme::RADIUS_LG)
                    .inner_margin(12.0),
            )
            .show(ctx, |ui| {
                prepare_module(ui);
                egui::ScrollArea::vertical().max_height(560.0).id_salt("module_scroll").show(
                    ui,
                    |ui| {
                        // Keep all content clear of the scrollbar.
                        let content_w = (ui.available_width() - panel::SCROLL_GUTTER).max(200.0);
                        ui.set_width(content_w);
                        prepare_module(ui);
                        // Mini histogram in every tool panel.
                        section(ui, "Levels");
                        if self.has_audio() {
                            self.histogram.show(ui, 96.0);
                        }
                        ui.add_space(6.0);
                        match module {
                            Module::Denoise => self.fx_denoise(ui, ctx),
                            Module::Dehum => self.fx_dehum(ui, ctx),
                            Module::Declick => self.fx_declick(ui, ctx),
                            Module::Decrackle => self.fx_decrackle(ui, ctx),
                            Module::Declip => self.fx_declip(ui, ctx),
                            Module::Deess => self.fx_deess(ui, ctx),
                            Module::Dereverb => self.fx_dereverb(ui, ctx),
                            Module::Dewind => self.fx_dewind(ui, ctx),
                            Module::Deplosive => self.fx_deplosive(ui, ctx),
                            Module::Derustle => self.fx_derustle(ui, ctx),
                            Module::Repair => self.fx_repair(ui, ctx),
                            Module::VoiceIsolate => self.fx_voice(ui, ctx),
                            Module::Breath => self.fx_breath(ui, ctx),
                            Module::Enhance => self.fx_enhance(ui, ctx),
                            Module::Inpaint => self.fx_inpaint(ui, ctx),
                            Module::Dewow => self.fx_dewow(ui, ctx),
                            Module::Azimuth => self.fx_azimuth(ui, ctx),
                            Module::Align => self.fx_align(ui, ctx),
                            Module::Dequantize => self.fx_dequantize(ui, ctx),
                            Module::Deemphasis => self.fx_deemphasis(ui, ctx),
                            Module::RiaaNormalize => self.fx_riaa_normalize(ui, ctx),
                            Module::Transform => self.fx_transform(ui, ctx),
                            Module::Hpss => self.fx_hpss(ui, ctx),
                            Module::Sms => self.fx_sms(ui, ctx),
                            Module::Selection => self.fx_selection(ui, ctx),
                            Module::Equalizer => self.fx_equalizer(ui, ctx),
                        }
                    },
                );
            });
        if !open {
            self.active_module = None;
        }
    }

    fn fx_denoise(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // Focus chips (RX Music Rebalance–style Voice/Bass/Drums/Other).
        section(ui, "Focus");
        let focus_id = ui.id().with("denoise_focus");
        let mut focus = ui.ctx().data_mut(|d| d.get_temp::<usize>(focus_id).unwrap_or(0));
        let chips = [
            (icons::MICROPHONE_STAGE, "Voice"),
            (icons::MUSIC_NOTE, "Bass"),
            (icons::MUSIC_NOTES, "Drums"),
            (icons::WAVEFORM, "Other"),
        ];
        if let Some(i) = stem_chips(ui, &chips, focus) {
            focus = i;
        }
        ui.ctx().data_mut(|d| d.insert_temp(focus_id, focus));
        ui.add_space(2.0);
        hint(
            ui,
            match focus {
                0 => "Voice — protect speech formants while cutting hiss.",
                1 => "Bass — keep the low end when reducing.",
                2 => "Drums — preserve transients / attacks.",
                _ => "Other — full-band broadband reduction.",
            },
        );

        section(ui, "Mixer");
        // One control per parameter — no dual fader/dial bindings.
        // Strip width ≈ 3×64 + 2×12 spacing; center in the module.
        let strip_w = 3.0 * 64.0 + 2.0 * 12.0;
        let pad = ((ui.available_width() - strip_w) * 0.5).max(0.0);
        ui.horizontal(|ui| {
            ui.add_space(pad);
            ui.spacing_mut().item_spacing.x = 12.0;
            // Strength (α): how hard the reducer works
            vertical_fader(
                ui,
                "dn_alpha",
                "Strength",
                &mut self.fx.denoise_alpha,
                1.0..=6.0,
                ValueFmt::Fixed(2),
                Color32::from_rgb(0, 180, 130),
            );
            // Spectral floor (β): residual left behind (shown as %)
            vertical_fader(
                ui,
                "dn_beta",
                "Floor",
                &mut self.fx.denoise_beta,
                0.0..=0.1,
                ValueFmt::Percent(1), // 0.010 → 1.0%
                Color32::from_rgb(50, 140, 230),
            );
            // Quiet-frame ratio for auto noise estimate (shown as %)
            vertical_fader(
                ui,
                "dn_ratio",
                "Noise",
                &mut self.fx.denoise_noise_ratio,
                0.05..=0.4,
                ValueFmt::Percent(0), // 0.15 → 15%
                Color32::from_rgb(160, 90, 220),
            );
        });
        ui.add_space(2.0);
        hint(ui, "Strength = reduction amount · Floor = residual · Noise = auto-print frames");

        section(ui, "Algorithm");
        check_row(ui, &mut self.fx.denoise_wiener, "Wiener filter (needs noise print)");
        check_row(ui, &mut self.fx.denoise_coherent, "Phase-coherent stereo");
        param_usize(ui, "FFT size", &mut self.fx.denoise_fft, 512..=4096);

        section(ui, "Noise print");
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if secondary_button(ui, "Learn from file").clicked() {
                if let Some(cur) = self.source_edit() {
                    match cathar::learn_noise_print(cur) {
                        Ok(np) => {
                            self.noise_print = Some(np);
                            self.status = "Noise print learned".into();
                        }
                        Err(e) => self.status = format!("noise print failed: {e}"),
                    }
                }
            }
            if self.noise_print.is_some() {
                ui.label(egui::RichText::new("✓ loaded").size(11.0).color(theme::ok()));
                if secondary_button(ui, "Clear").clicked() {
                    self.noise_print = None;
                }
            } else {
                ui.label(egui::RichText::new("auto min-stats").size(11.0).weak());
            }
        });
        hint(ui, "Learn a print from room tone, or use automatic quiet-frame stats.");
        let (preview, bypass, compare, render) = action_row(ui);
        if bypass {
            self.clear_preview(ctx);
            return;
        }
        if compare {
            self.toggle_compare(ctx);
            return;
        }
        if preview || render {
            let alpha = self.fx.denoise_alpha;
            let beta = self.fx.denoise_beta;
            let ratio = self.fx.denoise_noise_ratio;
            let fft = self.fx.denoise_fft.next_power_of_two().clamp(512, 8192);
            let hop = fft / 4;
            let wiener = self.fx.denoise_wiener;
            let coherent = self.fx.denoise_coherent;
            let np = self.noise_print.clone();
            let Some(cur) = self.source_edit().cloned() else { return };
            let out = if wiener {
                if let Some(ref print) = np {
                    cur.map_channels(|c| {
                        cathar::wiener_denoise(c, print, alpha).unwrap_or_else(|_| c.to_vec())
                    })
                } else {
                    self.status = "Wiener needs a learned noise print".into();
                    return;
                }
            } else {
                let d = SpectralDenoiser {
                    fft_size: fft,
                    hop_size: hop,
                    alpha,
                    beta,
                    noise_frame_ratio: ratio,
                    noise_print: np,
                };
                if coherent {
                    match d.denoise_coherent(&cur) {
                        Ok(o) => o,
                        Err(e) => {
                            self.status = format!("denoise failed: {e}");
                            return;
                        }
                    }
                } else {
                    match d.denoise(&cur) {
                        Ok(o) => o,
                        Err(e) => {
                            self.status = format!("denoise failed: {e}");
                            return;
                        }
                    }
                }
            };
            self.finish_fx(ctx, out, "denoise", render);
        }
    }

    fn fx_dehum(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Mains");
        param_f32(ui, "Base frequency (Hz)", &mut self.fx.hum_freq, 40.0..=120.0, 1);
        ui.horizontal(|ui| {
            if secondary_button(ui, "50 Hz").clicked() {
                self.fx.hum_freq = 50.0;
            }
            if secondary_button(ui, "60 Hz").clicked() {
                self.fx.hum_freq = 60.0;
            }
        });
        param_usize(ui, "Harmonics", &mut self.fx.hum_harmonics, 1..=12);
        section(ui, "Tracking");
        check_row(ui, &mut self.fx.hum_adaptive, "Adaptive (I/Q track drift)");
        hint(ui, "Adaptive follows slow pitch/amplitude wander in the hum.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-hum", |a, fx| {
            let (f, h, adaptive) = (fx.hum_freq, fx.hum_harmonics, fx.hum_adaptive);
            let sr = a.sample_rate;
            a.map_channels(|c| {
                if adaptive {
                    cathar::dehum_adaptive(c, sr, f, h)
                } else {
                    cathar::dehum(c, sr, f, h)
                }
            })
        });
    }

    fn fx_declick(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Detection");
        param_f32(ui, "Threshold × RMS", &mut self.fx.declick_threshold, 1.0..=12.0, 1);
        param_usize(ui, "Window (samples)", &mut self.fx.declick_window, 8..=512);
        hint(ui, "Lower threshold catches more clicks; higher is gentler.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-click", |a, fx| {
            let (t, w) = (fx.declick_threshold, fx.declick_window);
            a.map_channels(|c| cathar::declick(c, t, w))
        });
    }

    fn fx_declip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Clip detect");
        param_f32(ui, "Clip level", &mut self.fx.declip_threshold, 0.5..=1.0, 3);
        hint(ui, "Samples at/above this linear level are rebuilt (A-SPADE).");
        // Histogram helps set clip level.
        section(ui, "Level reference");
        if self.has_audio() {
            self.histogram.show(ui, 80.0);
        }
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-clip", |a, fx| {
            let t = fx.declip_threshold;
            a.map_channels(|c| cathar::declip(c, t))
        });
    }

    fn fx_deess(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Sibilance band");
        param_f32(ui, "Crossover (Hz)", &mut self.fx.deess_crossover, 3000.0..=12000.0, 0);
        param_f32(ui, "Threshold (dB)", &mut self.fx.deess_threshold_db, -60.0..=0.0, 1);
        param_f32(ui, "Ratio", &mut self.fx.deess_ratio, 1.0..=12.0, 1);
        section(ui, "Mode");
        check_row(ui, &mut self.fx.deess_multiband, "Multiband (adaptive per sub-band)");
        if self.fx.deess_multiband {
            param_usize(ui, "Bands", &mut self.fx.deess_bands, 2..=8);
        }
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-ess", |a, fx| {
            let (x, th, r) = (fx.deess_crossover, fx.deess_threshold_db, fx.deess_ratio);
            let multi = fx.deess_multiband;
            let bands = fx.deess_bands;
            let sr = a.sample_rate;
            a.map_channels(|c| {
                if multi {
                    cathar::deess_multiband(c, sr, x, th, r, bands)
                } else {
                    cathar::deesser(c, sr, x, th, r)
                }
            })
        });
    }

    fn fx_dereverb(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Mode");
        check_row(ui, &mut self.fx.dereverb_wpe, "WPE (weighted prediction error)");
        if self.fx.dereverb_wpe {
            section(ui, "WPE");
            param_usize(ui, "Taps (K)", &mut self.fx.dereverb_taps, 4..=40);
            param_usize(ui, "Delay (frames)", &mut self.fx.dereverb_delay, 1..=12);
            param_u32(ui, "Iterations", &mut self.fx.dereverb_iters, 1..=8);
        } else {
            section(ui, "Energy gate");
            param_f32(ui, "Strength", &mut self.fx.dereverb_strength, 0.0..=1.0, 2);
        }
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-reverb", |a, fx| {
            let sr = a.sample_rate;
            if fx.dereverb_wpe {
                let (taps, delay, iters) = (fx.dereverb_taps, fx.dereverb_delay, fx.dereverb_iters);
                a.map_channels(|c| cathar::wpe(c, sr, taps, delay, iters))
            } else {
                let s = fx.dereverb_strength;
                a.map_channels(|c| cathar::dereverb(c, sr, s))
            }
        });
    }

    fn fx_dewind(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "High-pass");
        param_f32(ui, "Cutoff (Hz)", &mut self.fx.dewind_cutoff, 20.0..=200.0, 0);
        hint(ui, "4th-order high-pass for wind rumble.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-wind", |a, fx| {
            let (sr, cut) = (a.sample_rate, fx.dewind_cutoff);
            a.map_channels(|c| cathar::dewind(c, sr, cut))
        });
    }

    fn fx_deplosive(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Plosive suppress");
        param_f32(ui, "Strength", &mut self.fx.deplosive_strength, 0.0..=1.0, 2);
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-plosive", |a, fx| {
            let (sr, s) = (a.sample_rate, fx.deplosive_strength);
            a.map_channels(|c| cathar::deplosive(c, sr, s))
        });
    }

    fn fx_derustle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Clothing / lav rustle");
        param_f32(ui, "Strength", &mut self.fx.derustle_strength, 0.0..=1.0, 2);
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-rustle", |a, fx| {
            let (sr, s) = (a.sample_rate, fx.derustle_strength);
            a.map_channels(|c| cathar::derustle(c, sr, s))
        });
    }

    fn fx_repair(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Spectral repair");
        param_f32(ui, "Strength", &mut self.fx.repair_strength, 0.0..=1.0, 2);
        hint(ui, "Pulls transient spectral outliers back to the temporal median.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "spectral repair", |a, fx| {
            let s = fx.repair_strength;
            a.map_channels(|c| cathar::spectral_repair(c, s))
        });
    }

    fn fx_voice(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Dialogue isolate");
        hint(ui, "Energy VAD + spectral gate. Optional noise print tightens the gate.");
        ui.horizontal(|ui| {
            if secondary_button(ui, "Learn noise print").clicked() {
                if let Some(cur) = self.source_edit() {
                    match cathar::learn_noise_print(cur) {
                        Ok(np) => {
                            self.noise_print = Some(np);
                            self.status = "Noise print learned".into();
                        }
                        Err(e) => self.status = format!("noise print failed: {e}"),
                    }
                }
            }
            if self.noise_print.is_some() {
                ui.label(egui::RichText::new("✓ print").size(11.0).color(theme::ok()));
            }
        });
        let (preview, bypass, compare, render) = action_row(ui);
        let np = self.noise_print.clone();
        self.run_fx(ctx, preview, bypass, compare, render, "voice isolate", |a, _fx| {
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::voice_isolate(c, sr, np.as_ref()))
        });
    }

    fn fx_breath(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Breath control");
        hint(ui, "Softens pre-speech breaths (not a hard gate).");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "breath", |a, _fx| {
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::breath_remove(c, sr))
        });
    }

    fn fx_enhance(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Bandwidth extension");
        param_u32(ui, "Target rate (Hz)", &mut self.fx.enhance_target_hz, 16_000..=96_000);
        let method_label = match self.fx.enhance_method {
            EnhanceMethod::Replicate => "SBR replicate",
            EnhanceMethod::Interpolate => "Log-mag interpolate",
        };
        egui::ComboBox::from_label("Method").selected_text(method_label).show_ui(ui, |ui| {
            ui.selectable_value(
                &mut self.fx.enhance_method,
                EnhanceMethod::Replicate,
                "SBR replicate",
            );
            ui.selectable_value(
                &mut self.fx.enhance_method,
                EnhanceMethod::Interpolate,
                "Log-mag interpolate",
            );
        });
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "enhance", |a, fx| {
            let target = fx.enhance_target_hz;
            let method = fx.enhance_method;
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::bandwidth_extend_with_method(c, sr, target, method))
        });
    }

    fn fx_dequantize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Source depth");
        param_u32(ui, "Source bits", &mut self.fx.dequant_bits, 4..=24);
        param_f32(ui, "Strength", &mut self.fx.dequant_strength, 0.0..=1.0, 2);
        hint(ui, "Lattice neighbour prediction (co-sparse depth is 0.8).");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "dequantize", |a, fx| {
            let (b, s) = (fx.dequant_bits, fx.dequant_strength);
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::dequantize(c, sr, b, s))
        });
    }

    fn fx_riaa_normalize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Vinyl RIAA");
        check_row(ui, &mut self.fx.riaa_elliptical, "Elliptical mono (rumble)");
        if self.fx.riaa_elliptical {
            param_f32(ui, "Crossover (Hz)", &mut self.fx.elliptical_hz, 50.0..=400.0, 0);
        }
        let (p1, b1, c1, r1) = action_row(ui);
        // Re-label mentally: this first row is for RIAA — we need two action groups.
        // Use the first row for RIAA only when render/preview on this block.
        if b1 {
            self.clear_preview(ctx);
        } else if c1 {
            self.toggle_compare(ctx);
        } else if p1 || r1 {
            let ell = self.fx.riaa_elliptical;
            let hz = self.fx.elliptical_hz;
            self.apply_whole(ctx, "RIAA", r1, move |a| {
                let sr = a.sample_rate;
                if a.channels.len() >= 2 && ell {
                    let (l, r) =
                        cathar::vinyl_restore(&a.channels[0], &a.channels[1], sr, Some(hz));
                    AudioData { sample_rate: sr, channels: vec![l, r] }
                } else {
                    a.map_channels(|c| cathar::riaa_deemphasis(c, sr))
                }
            });
        }

        section(ui, "Loudness");
        check_row(ui, &mut self.fx.normalize_use_peak, "Peak normalize (else EBU R128)");
        if self.fx.normalize_use_peak {
            param_f32(ui, "Target peak (dBFS)", &mut self.fx.normalize_peak_db, -12.0..=0.0, 1);
        } else {
            param_f32(ui, "Target LUFS", &mut self.fx.normalize_lufs, -30.0..=-6.0, 1);
            param_f32(ui, "Ceiling dBTP", &mut self.fx.normalize_ceiling, -6.0..=0.0, 1);
        }
        if render_button(ui, "Normalize").clicked() {
            if self.fx.normalize_use_peak {
                let pk = self.fx.normalize_peak_db;
                self.apply_whole(ctx, "normalize peak", true, move |a| {
                    a.map_channels(|c| cathar::normalize_peak(c, pk))
                });
            } else {
                let (l, c) = (self.fx.normalize_lufs, self.fx.normalize_ceiling);
                self.apply_whole(ctx, "normalize R128", true, move |a| a.normalize_r128(l, c));
            }
        }
    }

    fn fx_decrackle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Dense surface noise");
        param_f32(ui, "Sensitivity", &mut self.fx.decrackle_sensitivity, 1.0..=10.0, 1);
        hint(ui, "Laplacian detector over a running floor — denser than de-click.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-crackle", |a, fx| {
            let s = fx.decrackle_sensitivity;
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::decrackle(c, sr, s))
        });
    }

    fn fx_deemphasis(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Playback curve");
        let label = match self.fx.deemph_curve {
            cathar::Emphasis::Fm50 => "FM 50 µs",
            cathar::Emphasis::Fm75 => "FM 75 µs",
            cathar::Emphasis::CdIec => "CD / IEC 50/15 µs",
        };
        egui::ComboBox::from_label("Curve").selected_text(label).show_ui(ui, |ui| {
            ui.selectable_value(&mut self.fx.deemph_curve, cathar::Emphasis::Fm50, "FM 50 µs");
            ui.selectable_value(&mut self.fx.deemph_curve, cathar::Emphasis::Fm75, "FM 75 µs");
            ui.selectable_value(
                &mut self.fx.deemph_curve,
                cathar::Emphasis::CdIec,
                "CD / IEC 50/15 µs",
            );
        });
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-emphasis", |a, fx| {
            let curve = fx.deemph_curve;
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::deemphasis(c, sr, curve))
        });
    }

    fn fx_dewow(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Wow & flutter");
        hint(
            ui,
            "Tracks a dominant tone's instantaneous frequency and time-warps to flatten pitch. Best with a stable reference pitch.",
        );
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "de-wow", |a, _fx| {
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::dewow(c, sr))
        });
    }

    fn fx_inpaint(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Gap fill (AR / Janssen)");
        param_f32(ui, "Max auto gap (ms)", &mut self.fx.inpaint_max_gap_ms, 5.0..=200.0, 0);
        param_u32(ui, "Iterations", &mut self.fx.inpaint_iters, 1..=10);
        hint(ui, "Auto fills zero/NaN mutes; selection reconstructs a time span.");
        let has_sel = self.selection.is_some();
        let (preview, bypass, compare, render) = action_row(ui);
        if bypass {
            self.clear_preview(ctx);
            return;
        }
        if compare {
            self.toggle_compare(ctx);
            return;
        }
        if preview || render {
            if has_sel {
                if let Some(sel) = self.selection {
                    let (t0, t1) = (sel.t0, sel.t1);
                    let iters = self.fx.inpaint_iters;
                    self.apply_whole(ctx, "inpaint selection", render, move |a| {
                        let sr = a.sample_rate;
                        let start = (t0 * sr as f32) as usize;
                        let len = ((t1 - t0) * sr as f32) as usize;
                        a.map_channels(|c| cathar::inpaint_gap(c, start, len, iters))
                    });
                }
            } else {
                let max_ms = self.fx.inpaint_max_gap_ms;
                self.apply_whole(ctx, "inpaint auto", render, move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::inpaint_auto(c, sr, max_ms))
                });
            }
        }
        if has_sel {
            hint(ui, "Selection active → reconstruct time span on Preview/Render.");
        } else {
            hint(ui, "No selection → auto-fill mutes/dropouts.");
        }
    }

    fn fx_azimuth(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Stereo skew");
        param_f32(ui, "Max lag (ms)", &mut self.fx.azimuth_max_ms, 0.5..=20.0, 1);
        let stereo = self.stereo_file();
        if !stereo {
            hint(ui, "Requires a stereo file.");
        }
        let (preview, bypass, compare, render) = action_row(ui);
        if !stereo {
            return;
        }
        self.run_fx(ctx, preview, bypass, compare, render, "azimuth", |a, fx| {
            let max_ms = fx.azimuth_max_ms;
            if a.channels.len() >= 2 {
                let (l, r) =
                    cathar::azimuth_correct(&a.channels[0], &a.channels[1], a.sample_rate, max_ms);
                AudioData { sample_rate: a.sample_rate, channels: vec![l, r] }
            } else {
                a.clone()
            }
        });
    }

    fn fx_align(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Multi-mic align");
        param_f32(ui, "Max lag (ms)", &mut self.fx.align_max_ms, 1.0..=500.0, 0);
        hint(ui, "Aligns channel R to L (reference) by sub-sample cross-correlation.");
        let stereo = self.stereo_file();
        let (preview, bypass, compare, render) = action_row(ui);
        if !stereo {
            hint(ui, "Requires stereo (L = reference, R = to align).");
            return;
        }
        self.run_fx(ctx, preview, bypass, compare, render, "align", |a, fx| {
            let max_ms = fx.align_max_ms;
            if a.channels.len() >= 2 {
                let r = cathar::align(&a.channels[0], &a.channels[1], a.sample_rate, max_ms);
                AudioData { sample_rate: a.sample_rate, channels: vec![a.channels[0].clone(), r] }
            } else {
                a.clone()
            }
        });
    }

    fn fx_hpss(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Harmonic / percussive");
        param_usize(ui, "Median kernel", &mut self.fx.hpss_kernel, 3..=41);
        hint(ui, "Odd kernel preferred; even values are rounded up.");
        let mut keep_h = false;
        let mut keep_p = false;
        ui.horizontal(|ui| {
            keep_h = secondary_button(ui, "Keep harmonic").clicked();
            keep_p = secondary_button(ui, "Keep percussive").clicked();
        });
        let (preview, bypass, compare, render) = action_row(ui);
        if keep_h || ((preview || render) && !keep_p) {
            // Default render keeps harmonic if user hits Render without choosing.
            let k = self.fx.hpss_kernel | 1;
            let do_render = keep_h || render;
            if keep_h || preview || render {
                if bypass {
                    self.clear_preview(ctx);
                } else if compare {
                    self.toggle_compare(ctx);
                } else {
                    self.apply_whole(ctx, "HPSS harmonic", do_render && !preview, move |a| {
                        let sr = a.sample_rate;
                        a.map_channels(|c| cathar::hpss(c, sr, k).0)
                    });
                }
            }
        }
        if keep_p {
            let k = self.fx.hpss_kernel | 1;
            self.apply_whole(ctx, "HPSS percussive", true, move |a| {
                let sr = a.sample_rate;
                a.map_channels(|c| cathar::hpss(c, sr, k).1)
            });
        }
        // Handle bypass/compare from action row when not handled above.
        if bypass {
            self.clear_preview(ctx);
        } else if compare {
            self.toggle_compare(ctx);
        }
    }

    fn fx_sms(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Sinusoidal model");
        hint(ui, "Peak tracking + additive resynthesis — keeps tonal partials, drops residual.");
        let (preview, bypass, compare, render) = action_row(ui);
        self.run_fx(ctx, preview, bypass, compare, render, "SMS", |a, _fx| {
            let sr = a.sample_rate;
            a.map_channels(|c| cathar::synthesize_sms(&cathar::analyze_sms(c, sr)))
        });
    }

    fn fx_equalizer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.set_min_width(420.0);

        // Toolbar: On | preset | Flat  — same height, no fancy styling.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            ui.spacing_mut().interact_size.y = theme::CTRL_H;

            let mut on = self.fx.eq_enabled;
            if square_checkbox(ui, &mut on, "On").on_hover_text("Live EQ on playback").changed() {
                self.fx.eq_enabled = on;
                if on && self.preview.take().is_some() {
                    self.recompute(ctx);
                }
                self.apply_eq_to_engine();
            }

            ui.separator();

            let preset_label =
                EQ_PRESETS.get(self.fx.eq_preset).map(|p| p.name).unwrap_or("Custom");
            // NOTE: ComboBox::height() is the **popup menu max height**, not the
            // closed button. Using CTRL_H (30) here collapsed the list to one row.
            egui::ComboBox::from_id_salt("eq_preset_combo")
                .selected_text(egui::RichText::new(preset_label).size(13.5).color(theme::text()))
                .width(148.0)
                .height(360.0)
                .show_ui(ui, |ui| {
                    ui.set_min_width(168.0);
                    ui.spacing_mut().interact_size = egui::vec2(168.0, 36.0);
                    ui.spacing_mut().button_padding = egui::vec2(12.0, 10.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    let a = theme::accent();
                    ui.visuals_mut().selection.bg_fill =
                        Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), 50);
                    ui.visuals_mut().selection.stroke = Stroke::new(1.0, a);

                    for (i, p) in EQ_PRESETS.iter().enumerate() {
                        let label = egui::RichText::new(p.name).size(14.5).color(theme::text());
                        let r = ui.selectable_value(&mut self.fx.eq_preset, i, label);
                        if r.clicked() {
                            self.fx.eq_gains = p.gains;
                            self.status = p.blurb.into();
                            if self.fx.eq_enabled {
                                self.schedule_eq_reload();
                            }
                        }
                    }
                    ui.separator();
                    let custom = egui::RichText::new("Custom").size(14.5).color(theme::text());
                    if ui.selectable_label(self.fx.eq_preset >= EQ_PRESETS.len(), custom).clicked()
                    {
                        self.fx.eq_preset = EQ_PRESETS.len();
                    }
                });

            if ui
                .add(
                    egui::Button::new("Flat")
                        .min_size(egui::vec2(56.0, theme::CTRL_H))
                        .rounding(theme::RADIUS_MD),
                )
                .on_hover_text("Reset bands + preamp")
                .clicked()
            {
                self.fx.eq_gains = [0.0; 10];
                self.fx.eq_preamp = 0.0;
                self.fx.eq_preset = 0;
                if self.fx.eq_enabled {
                    self.apply_eq_to_engine();
                }
            }
        });

        ui.add_space(4.0);
        hint(
            ui,
            if self.fx.eq_enabled {
                "Live on playback — Render to bake into history."
            } else {
                "EQ off — turn On to audition."
            },
        );
        ui.add_space(6.0);

        // Fader bank: Preamp + 10 bands
        const FADER_H: f32 = 160.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // dB scale
            ui.vertical(|ui| {
                ui.set_width(32.0);
                ui.add_space(2.0);
                ui.label(egui::RichText::new("+12 dB").size(10.0).color(theme::text_muted()));
                ui.add_space(FADER_H * 0.5 - 18.0);
                ui.label(egui::RichText::new("  0 dB").size(10.0).color(theme::text_muted()));
                ui.add_space(FADER_H * 0.5 - 18.0);
                ui.label(egui::RichText::new("−12 dB").size(10.0).color(theme::text_muted()));
                ui.add_space(16.0);
            });

            // Preamp (iTunes left column)
            ui.vertical(|ui| {
                ui.set_width(36.0);
                ui.spacing_mut().slider_width = FADER_H;
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    let mut pre = self.fx.eq_preamp;
                    let r = ui.add_enabled(
                        self.fx.eq_enabled,
                        egui::Slider::new(&mut pre, EQ_GAIN_MIN..=EQ_GAIN_MAX)
                            .vertical()
                            .show_value(false),
                    );
                    if r.changed() {
                        self.fx.eq_preamp = pre;
                        self.fx.eq_preset = EQ_PRESETS.len();
                        self.schedule_eq_reload();
                    }
                    ui.label(
                        egui::RichText::new("Preamp")
                            .size(10.0)
                            .strong()
                            .color(theme::text_muted()),
                    );
                });
            });

            ui.separator();

            for (i, label) in EQ_BAND_LABELS.iter().enumerate() {
                ui.vertical(|ui| {
                    ui.set_width(32.0);
                    ui.spacing_mut().slider_width = FADER_H;
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        let mut g = self.fx.eq_gains[i];
                        let r = ui.add_enabled(
                            self.fx.eq_enabled,
                            egui::Slider::new(&mut g, EQ_GAIN_MIN..=EQ_GAIN_MAX)
                                .vertical()
                                .show_value(false),
                        );
                        if r.changed() {
                            self.fx.eq_gains[i] = g;
                            self.fx.eq_preset = EQ_PRESETS.len();
                            self.schedule_eq_reload();
                        }
                        ui.label(
                            egui::RichText::new(*label)
                                .size(10.5)
                                .strong()
                                .color(theme::text_muted()),
                        );
                    });
                });
            }
        });

        ui.add_space(8.0);
        // Live EQ: On/Off/Flat cover audition — only Compare + Render remain.
        let (compare, render) = compare_render_row(ui);
        if compare {
            self.toggle_compare(ctx);
            return;
        }
        if render {
            let gains = self.fx.eq_gains;
            let pre = self.fx.eq_preamp;
            // Disable live layer *before* bake so finish_fx does not double-apply.
            self.fx.eq_enabled = false;
            self.eq_needs_reload = false;
            self.run_fx(ctx, false, false, false, true, "EQ", move |a, _fx| {
                let sr = a.sample_rate;
                a.map_channels(|c| apply_eq_gains(c, sr, &gains, pre))
            });
            // Curve is in history — flat faders so On cannot stack on the commit.
            self.fx.eq_gains = [0.0; 10];
            self.fx.eq_preamp = 0.0;
            self.fx.eq_preset = 0;
        }
    }

    fn tick_visualizer(&mut self) {
        let playing = self.engine.as_ref().is_some_and(|e| e.is_playing());
        let pos = self.scrubbing.or_else(|| self.engine.as_ref().map(|e| e.pos())).unwrap_or(0.0);
        let sr = self.sample_rate;
        let mono: Vec<f32> = match self.displayed() {
            Some(a) if !a.channels.is_empty() => {
                // Mid mix for stereo
                if a.channels.len() >= 2 {
                    let l = &a.channels[0];
                    let r = &a.channels[1];
                    let n = l.len().max(r.len());
                    (0..n)
                        .map(|i| {
                            0.5 * (l.get(i).copied().unwrap_or(0.0)
                                + r.get(i).copied().unwrap_or(0.0))
                        })
                        .collect()
                } else {
                    a.channels[0].clone()
                }
            }
            _ => Vec::new(),
        };
        self.visualizer.tick(&mono, sr, pos, playing);
    }

    fn fx_transform(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Engine");
        let mode_label = match self.fx.stretch_mode {
            StretchMode::Wsola => "WSOLA",
            StretchMode::PhaseVocoder => "Phase vocoder",
        };
        egui::ComboBox::from_label("Mode").selected_text(mode_label).show_ui(ui, |ui| {
            ui.selectable_value(&mut self.fx.stretch_mode, StretchMode::Wsola, "WSOLA");
            ui.selectable_value(
                &mut self.fx.stretch_mode,
                StretchMode::PhaseVocoder,
                "Phase vocoder",
            );
        });
        section(ui, "Tempo (duration, pitch preserved)");
        param_f32(ui, "Factor", &mut self.fx.tempo_factor, 0.25..=4.0, 2);
        if render_button(ui, "Render tempo").clicked() {
            let f = self.fx.tempo_factor.max(0.01);
            let mode = self.fx.stretch_mode;
            self.apply_whole(ctx, "tempo", true, move |a| {
                let sr = a.sample_rate;
                a.map_channels(|c| cathar::time_stretch(c, sr, 1.0 / f, mode))
            });
        }
        section(ui, "Pitch (semitones, duration preserved)");
        param_f32(ui, "Semitones", &mut self.fx.pitch_semitones, -24.0..=24.0, 1);
        if render_button(ui, "Render pitch").clicked() {
            let st = self.fx.pitch_semitones;
            let mode = self.fx.stretch_mode;
            self.apply_whole(ctx, "pitch", true, move |a| {
                let sr = a.sample_rate;
                a.map_channels(|c| cathar::pitch_shift(c, sr, st, mode))
            });
        }
        section(ui, "Speed (resample — pitch + duration)");
        param_f32(ui, "Factor", &mut self.fx.speed_factor, 0.25..=4.0, 2);
        if render_button(ui, "Render speed").clicked() {
            let f = self.fx.speed_factor.max(0.01);
            self.apply_whole(ctx, "speed", true, move |a| {
                let sr = a.sample_rate;
                a.map_channels(|c| cathar::resample(c, (sr as f32 * f).round().max(1.0) as u32, sr))
            });
        }
        ui.add_space(8.0);
        if secondary_button(ui, "Bypass preview").clicked() {
            self.clear_preview(ctx);
        }
        if secondary_button(ui, "Compare original").clicked() {
            self.toggle_compare(ctx);
        }
    }

    fn fx_selection(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        section(ui, "Time–frequency selection");
        let has_sel = self.selection.is_some();
        if let Some(sel) = self.selection {
            ui.label(
                egui::RichText::new(format!(
                    "{:.3}–{:.3}s   ·   {:.0}–{:.0} Hz",
                    sel.t0, sel.t1, sel.f0, sel.f1
                ))
                .monospace()
                .size(11.5),
            );
        } else {
            hint(ui, "Drag on the spectrogram to select a region.");
        }
        section(ui, "Gain");
        param_f32(ui, "Gain (dB)", &mut self.gain_db, -60.0..=24.0, 1);
        section(ui, "Heal");
        param_f32(ui, "Heal strength", &mut self.fx.heal_strength, 0.0..=1.0, 2);
        let (preview, bypass, compare, render) = action_row(ui);
        if bypass {
            self.clear_preview(ctx);
            return;
        }
        if compare {
            self.toggle_compare(ctx);
            return;
        }
        if (preview || render) && has_sel {
            let g = 10f32.powf(self.gain_db / 20.0);
            // Prefer gain if non-zero change, else heal — both available via separate secondary.
            self.apply_selection(ctx, SpectralOp::Gain(g), "selection gain", render);
        }
        ui.horizontal(|ui| {
            if render_button_enabled(ui, has_sel, "Heal only").clicked() {
                self.apply_selection(ctx, SpectralOp::Heal, "heal", true);
            }
            if secondary_button(ui, "Clear selection").clicked() {
                self.selection = None;
            }
        });
    }

    fn central(&mut self, ctx: &egui::Context) {
        match self.viewer_mode {
            ViewerMode::Playlist => self.central_playlist(ctx),
            ViewerMode::Visualizer => self.central_visualizer(ctx),
            ViewerMode::Spectrogram => self.central_spectrogram(ctx),
        }
    }

    fn central_visualizer(&mut self, ctx: &egui::Context) {
        let well_bg = theme::well_bg();
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(well_bg).inner_margin(0.0))
            .show(ctx, |ui| {
                if !self.has_audio() {
                    // Same empty splash as spectrogram (logo + open hint).
                    let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::hover());
                    let rect = resp.rect;
                    painter.rect_filled(rect, 0.0, well_bg);
                    if let Some(logo) = &self.logo {
                        let [lw, lh] = logo.size();
                        let aspect = lw as f32 / lh as f32;
                        let target_h = (rect.height() * 0.35).min(220.0);
                        let target_w = target_h * aspect;
                        let logo_rect =
                            Rect::from_center_size(rect.center(), egui::vec2(target_w, target_h));
                        painter.image(
                            logo.id(),
                            logo_rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                        painter.text(
                            pos2(rect.center().x, logo_rect.bottom() + 20.0),
                            egui::Align2::CENTER_CENTER,
                            "Open an audio file  ·  File → Open",
                            egui::FontId::proportional(13.0),
                            theme::text_muted(),
                        );
                    } else {
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Open an audio file  ·  File → Open",
                            egui::FontId::proportional(15.0),
                            theme::text_muted(),
                        );
                    }
                    return;
                }
                let title = self.file_name.as_deref().unwrap_or("Untitled");
                self.visualizer.show(ui, title);
            });
    }

    fn central_playlist(&mut self, ctx: &egui::Context) {
        let well_bg = theme::well_bg();
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(well_bg)
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Playlist")
                            .size(16.0)
                            .strong()
                            .color(theme::text()),
                    );
                    ui.label(
                        egui::RichText::new(format!("· {} track(s)", self.playlist.len()))
                            .size(12.0)
                            .color(theme::text_muted()),
                    );
                    ui.add_space(12.0);
                    let mut auto = self.playlist_auto_advance;
                    if ui
                        .checkbox(&mut auto, "Auto-advance")
                        .on_hover_text("When a track ends, load and play the next in the queue")
                        .changed()
                    {
                        self.playlist_auto_advance = auto;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if secondary_button(ui, "Clear queue").clicked() {
                            self.playlist.clear();
                            self.playlist_sel = None;
                            self.status = "Playlist cleared".into();
                        }
                        if secondary_button(ui, "Import M3U…").clicked() {
                            self.pick_import_m3u(ctx);
                        }
                        if secondary_button(ui, "Add files…").clicked() {
                            self.pick_add_playlist(ctx);
                        }
                    });
                });
                ui.add_space(4.0);
                hint(
                    ui,
                    "Add files or import M3U. Double-click a row to edit. Transport ⏮/⏭ steps the queue.",
                );
                ui.add_space(8.0);

                if self.playlist.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.25);
                        ui.label(
                            egui::RichText::new(icons::MUSIC_NOTES)
                                .family(icons::family())
                                .size(42.0)
                                .color(theme::text_muted()),
                        );
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new("No tracks in the queue")
                                .size(15.0)
                                .color(theme::text_muted()),
                        );
                        ui.label(
                            egui::RichText::new("Add files to build a session playlist")
                                .size(12.0)
                                .color(theme::text_muted().gamma_multiply(0.85)),
                        );
                    });
                    return;
                }

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut load_idx: Option<usize> = None;
                        let mut remove_idx: Option<usize> = None;
                        for (i, entry) in self.playlist.iter().enumerate() {
                            let selected = self.playlist_sel == Some(i)
                                || self
                                    .file_path
                                    .as_ref()
                                    .is_some_and(|p| p == &entry.path);
                            let fill = if selected {
                                theme::accent().gamma_multiply(0.25)
                            } else {
                                theme::surface()
                            };
                            let stroke = if selected {
                                Stroke::new(1.0, theme::accent())
                            } else {
                                Stroke::new(1.0, theme::hairline())
                            };
                            let resp = egui::Frame::none()
                                .fill(fill)
                                .stroke(stroke)
                                .rounding(theme::RADIUS_LG)
                                .inner_margin(egui::Margin::symmetric(12.0, 10.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            egui::RichText::new(format!("{:>3}", i + 1))
                                                .monospace()
                                                .size(theme::FONT_LABEL)
                                                .color(theme::text_muted()),
                                        );
                                        ui.add_space(8.0);
                                        ui.vertical(|ui| {
                                            ui.label(
                                                egui::RichText::new(&entry.name)
                                                    .size(theme::FONT_BODY)
                                                    .strong()
                                                    .color(theme::text()),
                                            );
                                            ui.label(
                                                egui::RichText::new(entry.path.display().to_string())
                                                    .size(11.0)
                                                    .color(theme::text_muted()),
                                            );
                                        });
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui
                                                    .small_button("✕")
                                                    .on_hover_text("Remove from queue")
                                                    .clicked()
                                                {
                                                    remove_idx = Some(i);
                                                }
                                                if ui
                                                    .small_button("Open")
                                                    .on_hover_text("Load into editor")
                                                    .clicked()
                                                {
                                                    load_idx = Some(i);
                                                }
                                            },
                                        );
                                    });
                                })
                                .response
                                .interact(Sense::click());
                            if resp.double_clicked() {
                                load_idx = Some(i);
                            } else if resp.clicked() {
                                self.playlist_sel = Some(i);
                            }
                            ui.add_space(4.0);
                        }
                        if let Some(i) = remove_idx {
                            self.playlist.remove(i);
                            if self.playlist_sel == Some(i) {
                                self.playlist_sel = None;
                            } else if let Some(s) = self.playlist_sel {
                                if s > i {
                                    self.playlist_sel = Some(s - 1);
                                }
                            }
                        }
                        if let Some(i) = load_idx {
                            self.load_playlist_index(ctx, i);
                        }
                    });
            });
    }

    fn central_spectrogram(&mut self, ctx: &egui::Context) {
        let well_bg = theme::well_bg();
        // Pinned outside the scroll area (RX-style): never scroll horizontally to find it.
        // Only shown once a file is loaded — empty splash uses full width.
        const DB_BAR_W: f32 = 36.0;
        let show_db = self.has_audio();
        let db_w = if show_db { DB_BAR_W } else { 0.0 };

        egui::CentralPanel::default().frame(egui::Frame::none().fill(well_bg)).show(ctx, |ui| {
            let avail = ui.available_size();
            let spectro_h = avail.y.max(80.0);
            let view_w = (avail.x - db_w).max(64.0);
            let axis_text = theme::axis();
            // Image height inside the scroll content (time axis sits under it).
            let image_h_unscaled = (spectro_h - TIME_AXIS_H - 4.0).max(32.0);

            ui.horizontal(|ui| {
                ui.set_min_height(spectro_h);

                // Spectrogram scrolls; dB scale stays fixed on the right.
                ui.allocate_ui_with_layout(
                    egui::vec2(view_w, spectro_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::both()
                            .id_salt("spectro")
                            .drag_to_scroll(false)
                            .auto_shrink([false, false])
                            .max_width(view_w)
                            .max_height(spectro_h)
                            .show(ui, |ui| {
                                let image_w = view_w * self.zoom_x;
                                let image_h = image_h_unscaled * self.zoom_y;
                                let virt = egui::vec2(image_w + FREQ_AXIS_W, image_h + TIME_AXIS_H);
                                let (resp, painter) =
                                    ui.allocate_painter(virt, Sense::click_and_drag());
                                let outer = resp.rect;
                                let image = axes::spectro_image_rect(outer, image_w, image_h);

                                if let Some(tex) = &self.texture {
                                    painter.image(
                                        tex.id(),
                                        image,
                                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                        Color32::WHITE,
                                    );
                                } else {
                                    painter.rect_filled(image, 0.0, well_bg);
                                    if let Some(logo) = &self.logo {
                                        let [lw, lh] = logo.size();
                                        let aspect = lw as f32 / lh as f32;
                                        let target_h = (image.height() * 0.35).min(220.0);
                                        let target_w = target_h * aspect;
                                        let logo_rect = Rect::from_center_size(
                                            image.center(),
                                            egui::vec2(target_w, target_h),
                                        );
                                        painter.image(
                                            logo.id(),
                                            logo_rect,
                                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                                            Color32::WHITE,
                                        );
                                        painter.text(
                                            pos2(image.center().x, logo_rect.bottom() + 20.0),
                                            egui::Align2::CENTER_CENTER,
                                            "Open an audio file  ·  File → Open",
                                            egui::FontId::proportional(13.0),
                                            theme::text_muted(),
                                        );
                                    } else {
                                        painter.text(
                                            image.center(),
                                            egui::Align2::CENTER_CENTER,
                                            "Open an audio file",
                                            egui::FontId::proportional(18.0),
                                            theme::text_muted(),
                                        );
                                    }
                                }

                                if self.has_audio() && self.duration > 0.0 {
                                    axes::draw_freq_axis(
                                        &painter,
                                        outer,
                                        image,
                                        self.nyquist(),
                                        axis_text,
                                    );
                                    axes::draw_time_axis(
                                        &painter,
                                        outer,
                                        image,
                                        self.duration,
                                        axis_text,
                                    );
                                    let badge = match self.channel_view {
                                        ChannelView::Split if self.stereo_file() => "L  |  R",
                                        ChannelView::Left => "L",
                                        ChannelView::Right => "R",
                                        ChannelView::Mid => "L+R",
                                        ChannelView::Split => "L",
                                    };
                                    painter.text(
                                        pos2(image.left() + 8.0, image.top() + 6.0),
                                        egui::Align2::LEFT_TOP,
                                        badge,
                                        egui::FontId::proportional(12.0),
                                        theme::text().gamma_multiply(0.7),
                                    );
                                    if self.channel_view == ChannelView::Split && self.stereo_file()
                                    {
                                        painter.line_segment(
                                            [
                                                pos2(image.left(), image.center().y),
                                                pos2(image.right(), image.center().y),
                                            ],
                                            Stroke::new(1.0, theme::hairline()),
                                        );
                                        painter.text(
                                            pos2(image.left() + 8.0, image.center().y + 6.0),
                                            egui::Align2::LEFT_TOP,
                                            "R",
                                            egui::FontId::proportional(12.0),
                                            theme::text().gamma_multiply(0.7),
                                        );
                                    }
                                    self.handle_spectro_interaction(&resp, image);
                                    self.draw_selection(&painter, image);
                                    self.draw_playhead(&painter, image);
                                }
                            });
                    },
                );

                // Fixed dB colour bar — only with a loaded file (empty state is logo-only).
                if show_db {
                    let (bresp, bpaint) =
                        ui.allocate_painter(egui::vec2(DB_BAR_W, spectro_h), Sense::hover());
                    self.draw_db_bar(&bpaint, bresp.rect, axis_text);
                }
            });
        });
    }

    fn draw_db_bar(&self, painter: &egui::Painter, rect: Rect, text: Color32) {
        let inner = Rect::from_min_max(
            pos2(rect.left() + 6.0, rect.top() + 8.0),
            pos2(rect.left() + 14.0, rect.bottom() - TIME_AXIS_H - 8.0),
        );
        if inner.height() < 8.0 {
            return;
        }
        let steps = 48;
        for i in 0..steps {
            let t0 = i as f32 / steps as f32;
            let t1 = (i + 1) as f32 / steps as f32;
            // Top = hot (ceil), bottom = cold (floor) — matches spectro image.
            let y0 = inner.top() + t0 * inner.height();
            let y1 = inner.top() + t1 * inner.height();
            let level = 1.0 - (t0 + t1) * 0.5;
            let c = crate::colormap::cathar(level);
            painter.rect_filled(
                Rect::from_min_max(pos2(inner.left(), y0), pos2(inner.right(), y1)),
                0.0,
                c,
            );
        }
        painter.rect_stroke(inner, 0.0, Stroke::new(1.0, text.gamma_multiply(0.35)));
        let font = egui::FontId::proportional(9.0);
        painter.text(
            pos2(rect.right() - 2.0, inner.top()),
            egui::Align2::RIGHT_TOP,
            format!("{:.0}", self.db_ceil),
            font.clone(),
            text.gamma_multiply(0.75),
        );
        painter.text(
            pos2(rect.right() - 2.0, inner.bottom()),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.0}", self.db_floor),
            font,
            text.gamma_multiply(0.75),
        );
    }

    fn handle_spectro_interaction(&mut self, resp: &egui::Response, rect: Rect) {
        let dur = self.duration;
        let nyq = self.nyquist();
        let to_time = |x: f32| ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) * dur;
        let to_freq = |y: f32| (1.0 - ((y - rect.top()) / rect.height()).clamp(0.0, 1.0)) * nyq;

        if resp.drag_started() {
            self.drag_anchor = resp.interact_pointer_pos();
        }
        if resp.dragged() {
            if let (Some(a), Some(b)) = (self.drag_anchor, resp.interact_pointer_pos()) {
                self.selection = Some(Selection {
                    t0: to_time(a.x.min(b.x)),
                    t1: to_time(a.x.max(b.x)),
                    f0: to_freq(a.y.max(b.y)),
                    f1: to_freq(a.y.min(b.y)),
                });
            }
        }
        if resp.drag_stopped() {
            self.drag_anchor = None;
        }
        if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some(eng) = &self.engine {
                    eng.seek(to_time(p.x));
                }
            }
        }
    }

    fn draw_selection(&self, painter: &egui::Painter, rect: Rect) {
        let Some(sel) = self.selection else { return };
        let dur = self.duration;
        let nyq = self.nyquist();
        let x_at = |t: f32| rect.left() + (t / dur).clamp(0.0, 1.0) * rect.width();
        let y_at = |f: f32| rect.top() + (1.0 - (f / nyq).clamp(0.0, 1.0)) * rect.height();
        let sel_rect =
            Rect::from_min_max(pos2(x_at(sel.t0), y_at(sel.f1)), pos2(x_at(sel.t1), y_at(sel.f0)));
        painter.rect_filled(sel_rect, 0.0, theme::selection_fill());
        painter.rect_stroke(sel_rect, 0.0, Stroke::new(1.5, theme::selection_stroke()));
    }

    fn draw_playhead(&self, painter: &egui::Painter, rect: Rect) {
        if self.duration <= 0.0 {
            return;
        }
        let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
        let x = rect.left() + (pos / self.duration).clamp(0.0, 1.0) * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.5, theme::playhead()),
        );
    }

    fn draw_waveform_env(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        env: &[(f32, f32)],
        color: Color32,
    ) {
        if env.is_empty() {
            return;
        }
        let mid = rect.center().y;
        let half = rect.height() * 0.45;
        let n = env.len();
        for (i, &(lo, hi)) in env.iter().enumerate() {
            let x = rect.left() + i as f32 / n as f32 * rect.width();
            painter.line_segment(
                [pos2(x, mid - hi * half), pos2(x, mid - lo * half)],
                Stroke::new(1.0, color),
            );
        }
    }
}

/// Decode the bundled logo for the empty-state splash.
///
/// The asset is drawn for a dark navy canvas (white metal + terracotta). In light
/// mode the white geometry would vanish on paper, so we recolor near-white /
/// silver ink to dark charcoal while preserving the warm orange accents.
fn load_logo(ctx: &egui::Context, for_light: bool) -> Option<TextureHandle> {
    let bytes = include_bytes!("../../../docs/logo.png");
    let mut img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let bg = *img.get_pixel(0, 0);
    let near = |a: u8, b: u8| (a as i32 - b as i32).abs() < 44;
    // Dark charcoal for light-mode “metal” strokes (reads on paper).
    const INK: [u8; 4] = [42, 36, 32, 255];
    for px in img.pixels_mut() {
        // Key out the baked dark background (corner sample).
        if near(px[0], bg[0]) && near(px[1], bg[1]) && near(px[2], bg[2]) {
            px[3] = 0;
            continue;
        }
        if !for_light || px[3] < 16 {
            continue;
        }
        let r = px[0] as i32;
        let g = px[1] as i32;
        let b = px[2] as i32;
        let lum = (r + g + b) / 3;
        // Terracotta / warm accent — leave alone.
        let warm = r > g + 25 && r > b + 25 && r > 100;
        // White / silver / light grey metal of the mark (would vanish on paper).
        let cool_metal = !warm && lum > 120 && (r - g).abs() < 45 && (g - b).abs() < 55;
        if cool_metal {
            // Map bright metal → ink; keep alpha for soft edges.
            let t = ((lum - 120) as f32 / 135.0).clamp(0.0, 1.0);
            let mix = |c: u8, i: u8| ((c as f32) * (1.0 - t) + (i as f32) * t).round() as u8;
            px[0] = mix(px[0], INK[0]);
            px[1] = mix(px[1], INK[1]);
            px[2] = mix(px[2], INK[2]);
            if lum > 170 {
                px[0] = INK[0];
                px[1] = INK[1];
                px[2] = INK[2];
            }
        }
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    // Distinct texture name so light/dark variants don’t share a stale cache key.
    let name = if for_light { "logo_light" } else { "logo_dark" };
    Some(ctx.load_texture(name, color, TextureOptions::LINEAR))
}

fn display_hop(len: usize) -> usize {
    if len <= FFT_SIZE {
        return HOP;
    }
    (len - FFT_SIZE).div_ceil(MAX_COLS - 1).max(HOP)
}

/// Player-bar clock: fixed shape so digits don’t reflow the row.
/// Under 1h → always `MM:SS.s`; with hours → `H:MM:SS`.
fn fmt_time_player(secs: f32) -> String {
    let secs = secs.max(0.0);
    let h = (secs / 3600.0).floor() as u32;
    let m = ((secs % 3600.0) / 60.0).floor() as u32;
    let s = secs % 60.0;
    if h > 0 {
        let s_whole = s.floor() as u32;
        format!("{h}:{m:02}:{s_whole:02}")
    } else {
        // Always two-digit minutes so "00:42.2" and "03:55.0" share width.
        format!("{m:02}:{s:04.1}")
    }
}

/// General HH:MM:SS.mmm (status / diagnostics).
#[allow(dead_code)]
fn fmt_time_hms(secs: f32) -> String {
    let secs = secs.max(0.0);
    let h = (secs / 3600.0).floor() as u32;
    let m = ((secs % 3600.0) / 60.0).floor() as u32;
    let s = secs % 60.0;
    if h > 0 { format!("{h}:{m:02}:{s:06.3}") } else { format!("{m:02}:{s:06.3}") }
}

/// Peak absolute sample in a window centred on `pos_sec` (seconds).
fn live_peak(ch: &[f32], pos_sec: f32, sample_rate: u32, half_win: usize) -> f32 {
    if ch.is_empty() || sample_rate == 0 {
        return 0.0;
    }
    let i = (pos_sec.max(0.0) * sample_rate as f32).round() as usize;
    let i = i.min(ch.len().saturating_sub(1));
    let start = i.saturating_sub(half_win);
    let end = (i + half_win).min(ch.len());
    if start >= end {
        return 0.0;
    }
    ch[start..end].iter().map(|s| s.abs()).fold(0.0f32, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_hop_bounds_texture_width() {
        assert_eq!(display_hop(1000), HOP);
        assert_eq!(display_hop(FFT_SIZE), HOP);
        for &len in &[1_000_000usize, 10_070_016, 48_000 * 1800] {
            let hop = display_hop(len);
            let frames = (len - FFT_SIZE) / hop + 1;
            assert!(frames <= MAX_COLS, "len {len}: {frames} cols > {MAX_COLS}");
        }
    }
}
