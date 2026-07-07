//! The Cathar spectral editor application.

use crate::axes::{self, FREQ_AXIS_W, TIME_AXIS_H};
use crate::engine::Engine;
use crate::icons::{self, MenuItem, rich, toolbar_button, toolbar_toggle, transport_button};
use crate::panel::{
    self, apply_button, apply_button_enabled, column_heading, fx_section, hint, param_f32,
    param_u32, param_usize,
};
use crate::spectral_edit::{Selection, SpectralOp, apply_spectral};
use crate::spectro::{colorize, compute_spectrogram, mono_mix, waveform_envelope};
use crate::theme::{self, Appearance};
use cathar::{AudioData, Denoiser, SpectralDenoiser, Spectrogram};
use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, Theme, pos2};

const FFT_SIZE: usize = 2048;
const HOP: usize = 512;
/// Max spectrogram columns (GPU texture side limit is 16384; leave margin).
/// Long files widen the display hop so the texture never exceeds this.
const MAX_COLS: usize = 16000;

/// User-adjustable parameters for each whole-file restoration stage.
struct FxParams {
    denoise_alpha: f32,
    denoise_beta: f32,
    hum_freq: f32,
    hum_harmonics: usize,
    declick_threshold: f32,
    declick_window: usize,
    declip_threshold: f32,
    deess_crossover: f32,
    deess_threshold_db: f32,
    deess_ratio: f32,
    dereverb_strength: f32,
    dereverb_wpe: bool,
    dereverb_taps: usize,
    dequant_bits: u32,
    dequant_strength: f32,
    normalize_lufs: f32,
    normalize_ceiling: f32,
    hum_adaptive: bool,
    decrackle_sensitivity: f32,
    deemph_curve: cathar::Emphasis,
    hpss_kernel: usize,
    tempo_factor: f32,
    pitch_semitones: f32,
    speed_factor: f32,
}

impl Default for FxParams {
    fn default() -> Self {
        Self {
            denoise_alpha: 3.0,
            denoise_beta: 0.01,
            hum_freq: 60.0,
            hum_harmonics: 4,
            declick_threshold: 5.0,
            declick_window: 64,
            declip_threshold: 0.95,
            deess_crossover: 6000.0,
            deess_threshold_db: -30.0,
            deess_ratio: 4.0,
            dereverb_strength: 0.5,
            dereverb_wpe: false,
            dereverb_taps: 15,
            dequant_bits: 16,
            dequant_strength: 0.7,
            normalize_lufs: -14.0,
            normalize_ceiling: -1.0,
            hum_adaptive: false,
            decrackle_sensitivity: 5.0,
            deemph_curve: cathar::Emphasis::Fm50,
            hpss_kernel: 17,
            tempo_factor: 1.0,
            pitch_semitones: 0.0,
            speed_factor: 1.0,
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
    /// Cached STFT of the displayed buffer, so display-window changes only
    /// re-colour rather than recompute.
    spec: Option<Spectrogram>,
    sample_rate: u32,
    duration: f32,
    waveform: Vec<(f32, f32)>,
    /// Current selection in physical units (seconds, Hz).
    selection: Option<Selection>,
    drag_anchor: Option<Pos2>,
    show_original: bool,
    gain_db: f32,
    /// Spectrogram display window (dB) — the gain/contrast control.
    db_floor: f32,
    db_ceil: f32,
    /// Spectrogram zoom (× fit) in time and frequency.
    zoom_x: f32,
    zoom_y: f32,
    fx: FxParams,
    appearance: Appearance,
    /// Last resolved theme — used to re-apply custom styling when the OS toggles.
    resolved_theme: Theme,
    status: String,
}

impl CatharGui {
    /// Build the app; opens the audio device if one is available.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        crate::fonts::install(&cc.egui_ctx);
        // Default to the polished dark theme (the pro-audio look); Light/System
        // remain available from the View menu.
        let appearance = Appearance::Dark;
        theme::apply(&cc.egui_ctx, appearance);
        let resolved_theme = theme::resolved(&cc.egui_ctx, appearance);
        let logo = load_logo(&cc.egui_ctx);
        let engine = Engine::new().ok();
        let status = match &engine {
            Some(_) => "Open an audio file to begin (File → Open).".to_string(),
            None => "No audio output device — editing works, playback is disabled.".to_string(),
        };
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
            waveform: Vec::new(),
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
        }
    }

    /// Re-apply custom theme when following the OS and light/dark flips.
    fn sync_system_theme(&mut self, ctx: &egui::Context) {
        if self.appearance != Appearance::System {
            return;
        }
        let now = theme::resolved(ctx, Appearance::System);
        if now != self.resolved_theme {
            self.resolved_theme = now;
            theme::apply(ctx, Appearance::System);
        }
    }

    fn has_audio(&self) -> bool {
        !self.history.is_empty()
    }

    /// The buffer currently shown (original when A/B is toggled to original).
    fn displayed(&self) -> Option<&AudioData> {
        if self.show_original { self.original.as_ref() } else { self.history.get(self.hist_idx) }
    }

    fn open(&mut self, ctx: &egui::Context, path: String) {
        match AudioData::from_file(&path) {
            Ok(audio) => {
                self.sample_rate = audio.sample_rate;
                self.original = Some(audio.clone());
                if let Some(eng) = &mut self.engine {
                    let _ = eng.load(&audio);
                }
                self.history = vec![audio];
                self.hist_idx = 0;
                self.show_original = false;
                self.selection = None;
                self.status = format!("Loaded {path}");
                self.recompute(ctx);
            }
            Err(e) => self.status = format!("Failed to open: {e}"),
        }
    }

    fn save(&mut self, path: String) {
        let Some(audio) = self.history.get(self.hist_idx) else { return };
        match audio.to_file(&path) {
            Ok(()) => self.status = format!("Saved {path}"),
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    /// Recompute STFT + waveform from the displayed buffer, then colour it.
    fn recompute(&mut self, ctx: &egui::Context) {
        let Some(audio) = self.displayed() else { return };
        let sr = audio.sample_rate;
        let mono = mono_mix(audio);
        self.duration = if sr > 0 { mono.len() as f32 / sr as f32 } else { 0.0 };
        self.waveform = waveform_envelope(&mono, 2000);
        self.spec = Some(compute_spectrogram(&mono, sr, FFT_SIZE, display_hop(mono.len())));
        self.recolor(ctx);
    }

    /// Rebuild the texture from the cached STFT and the current display window.
    /// Cheap — used when only `db_floor`/`db_ceil` change.
    fn recolor(&mut self, ctx: &egui::Context) {
        let img = match &self.spec {
            Some(spec) => colorize(spec, self.db_floor, self.db_ceil),
            None => return,
        };
        // LINEAR (bilinear) filtering: the bins-tall texture is stretched to the
        // panel, so NEAREST produced hard vertical comb banding between frequency
        // rows and blocky columns when zoomed in. LINEAR interpolates both axes
        // for a smooth spectrogram.
        self.texture = Some(ctx.load_texture("spectrogram", img, TextureOptions::LINEAR));
    }

    /// Commit a new buffer as an edit: truncates any redo tail, reloads playback.
    fn push_edit(&mut self, ctx: &egui::Context, audio: AudioData) {
        self.history.truncate(self.hist_idx + 1);
        self.history.push(audio);
        self.hist_idx = self.history.len() - 1;
        self.show_original = false;
        if let Some(eng) = &mut self.engine {
            let _ = eng.load(&self.history[self.hist_idx]);
        }
        self.recompute(ctx);
    }

    fn apply_whole<F: FnOnce(&AudioData) -> AudioData>(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        f: F,
    ) {
        let Some(cur) = self.history.get(self.hist_idx) else { return };
        let new = f(cur);
        self.push_edit(ctx, new);
        self.status = format!("Applied {label}");
    }

    fn apply_selection(&mut self, ctx: &egui::Context, op: SpectralOp, label: &str) {
        let (Some(sel), Some(cur)) = (self.selection, self.history.get(self.hist_idx)) else {
            self.status = "Draw a selection on the spectrogram first.".into();
            return;
        };
        let sr = cur.sample_rate;
        let new = cur.map_channels(|c| apply_spectral(c, sr, &sel, op));
        self.push_edit(ctx, new);
        self.status = format!("{label} on selection");
    }

    fn undo(&mut self, ctx: &egui::Context) {
        if self.hist_idx > 0 {
            self.hist_idx -= 1;
            self.show_original = false;
            if let Some(eng) = &mut self.engine {
                let _ = eng.load(&self.history[self.hist_idx]);
            }
            self.recompute(ctx);
            self.status = "Undo".into();
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if self.hist_idx + 1 < self.history.len() {
            self.hist_idx += 1;
            self.show_original = false;
            if let Some(eng) = &mut self.engine {
                let _ = eng.load(&self.history[self.hist_idx]);
            }
            self.recompute(ctx);
            self.status = "Redo".into();
        }
    }

    fn nyquist(&self) -> f32 {
        self.sample_rate as f32 / 2.0
    }
}

impl eframe::App for CatharGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Keyboard shortcuts: Space = play/pause, Z = undo, Y = redo.
        let (toggle, do_undo, do_redo) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Z),
                i.key_pressed(egui::Key::Y),
            )
        });
        if toggle {
            if let Some(eng) = &self.engine {
                eng.toggle();
            }
        }
        if do_undo {
            self.undo(ctx);
        }
        if do_redo {
            self.redo(ctx);
        }

        self.sync_system_theme(ctx);

        self.top_bar(ctx);
        self.side_panel(ctx);
        self.central(ctx);

        // Keep the playhead live while playing.
        if let Some(eng) = &self.engine {
            if !eng.is_paused() {
                ctx.request_repaint();
            }
        }
    }
}

impl CatharGui {
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("chrome").show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.add(MenuItem::new(icons::FOLDER_OPEN, "Open…")).clicked() {
                        ui.close_menu();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter(
                                "Audio",
                                &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif"],
                            )
                            .pick_file()
                        {
                            self.open(ctx, p.display().to_string());
                        }
                    }
                    if ui
                        .add_enabled(self.has_audio(), MenuItem::new(icons::FLOPPY_DISK, "Save…"))
                        .clicked()
                    {
                        ui.close_menu();
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Audio", &["wav", "flac", "aiff"])
                            .set_file_name("edited.wav")
                            .save_file()
                        {
                            self.save(p.display().to_string());
                        }
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            self.hist_idx > 0,
                            MenuItem::new(icons::ARROW_COUNTER_CLOCKWISE, "Undo"),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.undo(ctx);
                    }
                    if ui
                        .add_enabled(
                            self.hist_idx + 1 < self.history.len(),
                            MenuItem::new(icons::ARROWS_CLOCKWISE, "Redo"),
                        )
                        .clicked()
                    {
                        ui.close_menu();
                        self.redo(ctx);
                    }
                });

                ui.menu_button("View", |ui| {
                    let mut mode = self.appearance;
                    ui.label("Theme");
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut mode,
                            Appearance::System,
                            rich(icons::DESKTOP, icons::ICON_SIZE),
                        )
                        .on_hover_text("System");
                        ui.selectable_value(
                            &mut mode,
                            Appearance::Light,
                            rich(icons::SUN, icons::ICON_SIZE),
                        )
                        .on_hover_text("Light");
                        ui.selectable_value(
                            &mut mode,
                            Appearance::Dark,
                            rich(icons::MOON, icons::ICON_SIZE),
                        )
                        .on_hover_text("Dark");
                    });
                    if mode != self.appearance {
                        self.appearance = mode;
                        theme::apply(ctx, self.appearance);
                        self.resolved_theme = theme::resolved(ctx, self.appearance);
                    }
                    ui.separator();
                    if ui.button("Reset spectrogram zoom").clicked() {
                        self.zoom_x = 1.0;
                        self.zoom_y = 1.0;
                        ui.close_menu();
                    }
                });
            });

            ui.separator();
            ui.add_space(2.0);

            // Transport — left-aligned like Logic, not centered in a void.
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing.x = 8.0;

                let playing = self.engine.as_ref().map(|e| !e.is_paused()).unwrap_or(false);
                let play_icon = if playing { icons::PAUSE } else { icons::PLAY };
                let play_tip = if playing { "Pause (Space)" } else { "Play (Space)" };
                if ui
                    .add_enabled(self.has_audio(), transport_button(play_icon))
                    .on_hover_text(play_tip)
                    .clicked()
                {
                    if let Some(eng) = &self.engine {
                        eng.toggle();
                    }
                }

                let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{}  /  {}",
                        fmt_time(pos),
                        fmt_time(self.duration)
                    ))
                    .monospace()
                    .size(13.0),
                );

                ui.separator();
                ui.add_space(2.0);

                if ui
                    .add_enabled(self.hist_idx > 0, toolbar_button(icons::ARROW_COUNTER_CLOCKWISE))
                    .on_hover_text("Undo (Z)")
                    .clicked()
                {
                    self.undo(ctx);
                }
                if ui
                    .add_enabled(
                        self.hist_idx + 1 < self.history.len(),
                        toolbar_button(icons::ARROWS_CLOCKWISE),
                    )
                    .on_hover_text("Redo (Y)")
                    .clicked()
                {
                    self.redo(ctx);
                }

                let ab_enabled = self.original.is_some();
                let mut ab = self.show_original;
                if ui
                    .add_enabled(ab_enabled, toolbar_toggle(ab, icons::SWAP))
                    .on_hover_text("Compare original")
                    .clicked()
                {
                    ab = !ab;
                    self.show_original = ab;
                    self.recompute(ctx);
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(10.0);
                });
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(&self.status);
                if let Some(sel) = self.selection {
                    ui.separator();
                    ui.label(format!(
                        "sel: {:.2}–{:.2}s · {:.0}–{:.0} Hz",
                        sel.t0, sel.t1, sel.f0, sel.f1
                    ));
                }
            });
        });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("tools")
            .default_width(panel::INSPECTOR_W)
            .width_range(260.0..=panel::INSPECTOR_W)
            .resizable(true)
            .show(ctx, |ui| {
                let panel_w = ui.available_width();
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    panel::prepare(ui, panel_w);
                    let enabled = self.has_audio();
                    column_heading(ui, "Restoration");
                    ui.add_enabled_ui(enabled, |ui| {
                        self.fx_denoise(ui, ctx);
                        self.fx_dehum(ui, ctx);
                        self.fx_declick(ui, ctx);
                        self.fx_decrackle(ui, ctx);
                        self.fx_declip(ui, ctx);
                        self.fx_deess(ui, ctx);
                        self.fx_dereverb(ui, ctx);
                        self.fx_inpaint(ui, ctx);
                        self.fx_dewow(ui, ctx);
                        self.fx_azimuth(ui, ctx);
                        self.fx_dequantize(ui, ctx);
                        self.fx_deemphasis(ui, ctx);
                        self.fx_riaa_normalize(ui, ctx);
                    });

                    column_heading(ui, "Transform & separate");
                    ui.add_enabled_ui(enabled, |ui| {
                        self.fx_transform(ui, ctx);
                        self.fx_hpss(ui, ctx);
                        self.fx_sms(ui, ctx);
                    });

                    column_heading(ui, "Selection");
                    let has_sel = self.selection.is_some();
                    ui.add_enabled_ui(enabled && has_sel, |ui| {
                        param_f32(ui, "Gain", &mut self.gain_db, -60.0..=24.0, 1);
                        if apply_button(ui, "Apply gain").clicked() {
                            let g = 10f32.powf(self.gain_db / 20.0);
                            self.apply_selection(ctx, SpectralOp::Gain(g), "Gain");
                        }
                        if apply_button(ui, "Heal selection").clicked() {
                            self.apply_selection(ctx, SpectralOp::Heal, "Heal");
                        }
                    });
                    if apply_button_enabled(ui, has_sel, "Clear selection").clicked() {
                        self.selection = None;
                    }

                    column_heading(ui, "Display");
                    let r1 = param_f32(ui, "dB floor", &mut self.db_floor, -120.0..=-30.0, 0);
                    let r2 = param_f32(ui, "dB ceiling", &mut self.db_ceil, -30.0..=6.0, 0);
                    if r1.changed() || r2.changed() {
                        self.recolor(ctx);
                    }
                    param_f32(ui, "Zoom time", &mut self.zoom_x, 1.0..=12.0, 1);
                    param_f32(ui, "Zoom frequency", &mut self.zoom_y, 1.0..=8.0, 1);
                    if apply_button(ui, "Reset view").clicked() {
                        self.zoom_x = 1.0;
                        self.zoom_y = 1.0;
                    }

                    ui.add_space(8.0);
                    hint(
                        ui,
                        "Drag on the spectrogram to select. Scroll to pan when zoomed. \
                     Click the waveform to seek.",
                    );
                });
            });
    }

    fn fx_denoise(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "denoise", "Denoise", |ui| {
            param_f32(ui, "Strength α", &mut self.fx.denoise_alpha, 1.0..=6.0, 2);
            param_f32(ui, "Floor β", &mut self.fx.denoise_beta, 0.0..=0.1, 3);
            if apply_button(ui, "Apply denoise").clicked() {
                let (alpha, beta) = (self.fx.denoise_alpha, self.fx.denoise_beta);
                let cur = self.history[self.hist_idx].clone();
                let d = SpectralDenoiser { alpha, beta, ..Default::default() };
                match d.denoise(&cur) {
                    Ok(out) => {
                        self.push_edit(ctx, out);
                        self.status = "Applied denoise".into();
                    }
                    Err(e) => self.status = format!("denoise failed: {e}"),
                }
            }
        });
    }

    fn fx_dehum(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "dehum", "De-hum", |ui| {
            param_f32(ui, "Base frequency", &mut self.fx.hum_freq, 40.0..=120.0, 0);
            param_usize(ui, "Harmonics", &mut self.fx.hum_harmonics, 1..=10);
            ui.checkbox(&mut self.fx.hum_adaptive, "Adaptive (track drift)");
            if apply_button(ui, "Apply de-hum").clicked() {
                let (f, h, adaptive) =
                    (self.fx.hum_freq, self.fx.hum_harmonics, self.fx.hum_adaptive);
                self.apply_whole(ctx, "de-hum", move |a| {
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
        });
    }

    fn fx_declick(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "declick", "De-click", |ui| {
            param_f32(ui, "Threshold", &mut self.fx.declick_threshold, 1.0..=12.0, 1);
            param_usize(ui, "Window", &mut self.fx.declick_window, 8..=256);
            if apply_button(ui, "Apply de-click").clicked() {
                let (t, w) = (self.fx.declick_threshold, self.fx.declick_window);
                self.apply_whole(ctx, "de-click", move |a| {
                    a.map_channels(|c| cathar::declick(c, t, w))
                });
            }
        });
    }

    fn fx_declip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "declip", "De-clip", |ui| {
            param_f32(ui, "Clip level", &mut self.fx.declip_threshold, 0.5..=1.0, 2);
            if apply_button(ui, "Apply de-clip").clicked() {
                let t = self.fx.declip_threshold;
                self.apply_whole(ctx, "de-clip", move |a| a.map_channels(|c| cathar::declip(c, t)));
            }
        });
    }

    fn fx_deess(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "deess", "De-ess", |ui| {
            param_f32(ui, "Crossover", &mut self.fx.deess_crossover, 3000.0..=12000.0, 0);
            param_f32(ui, "Threshold", &mut self.fx.deess_threshold_db, -60.0..=0.0, 1);
            param_f32(ui, "Ratio", &mut self.fx.deess_ratio, 1.0..=10.0, 1);
            if apply_button(ui, "Apply de-ess").clicked() {
                let (x, th, r) =
                    (self.fx.deess_crossover, self.fx.deess_threshold_db, self.fx.deess_ratio);
                self.apply_whole(ctx, "de-ess", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::deesser(c, sr, x, th, r))
                });
            }
        });
    }

    fn fx_dereverb(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "dereverb", "De-reverb", |ui| {
            ui.checkbox(&mut self.fx.dereverb_wpe, "WPE (linear prediction)");
            if self.fx.dereverb_wpe {
                param_usize(ui, "Taps", &mut self.fx.dereverb_taps, 4..=30);
            } else {
                param_f32(ui, "Strength", &mut self.fx.dereverb_strength, 0.0..=1.0, 2);
            }
            if apply_button(ui, "Apply de-reverb").clicked() {
                if self.fx.dereverb_wpe {
                    let taps = self.fx.dereverb_taps;
                    self.apply_whole(ctx, "de-reverb (WPE)", move |a| {
                        let sr = a.sample_rate;
                        a.map_channels(|c| cathar::wpe(c, sr, taps, 3, 3))
                    });
                } else {
                    let s = self.fx.dereverb_strength;
                    self.apply_whole(ctx, "de-reverb", move |a| {
                        let sr = a.sample_rate;
                        a.map_channels(|c| cathar::dereverb(c, sr, s))
                    });
                }
            }
        });
    }

    fn fx_dequantize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "dequantize", "Dequantize", |ui| {
            param_u32(ui, "Source bits", &mut self.fx.dequant_bits, 4..=24);
            param_f32(ui, "Strength", &mut self.fx.dequant_strength, 0.0..=1.0, 2);
            if apply_button(ui, "Apply dequantize").clicked() {
                let (b, s) = (self.fx.dequant_bits, self.fx.dequant_strength);
                self.apply_whole(ctx, "dequantize", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::dequantize(c, sr, b, s))
                });
            }
        });
    }

    fn fx_riaa_normalize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "riaa", "RIAA / Normalize", |ui| {
            if apply_button(ui, "RIAA de-emphasis").clicked() {
                self.apply_whole(ctx, "RIAA", |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::riaa_deemphasis(c, sr))
                });
            }
            ui.add_space(8.0);
            param_f32(ui, "Target LUFS", &mut self.fx.normalize_lufs, -30.0..=-6.0, 1);
            param_f32(ui, "Ceiling dBTP", &mut self.fx.normalize_ceiling, -6.0..=0.0, 1);
            if apply_button(ui, "Normalize").clicked() {
                let (l, c) = (self.fx.normalize_lufs, self.fx.normalize_ceiling);
                self.apply_whole(ctx, "normalize", move |a| a.normalize_r128(l, c));
            }
        });
    }

    fn fx_decrackle(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "decrackle", "De-crackle", |ui| {
            param_f32(ui, "Sensitivity", &mut self.fx.decrackle_sensitivity, 1.0..=10.0, 1);
            if apply_button(ui, "Apply de-crackle").clicked() {
                let s = self.fx.decrackle_sensitivity;
                self.apply_whole(ctx, "de-crackle", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::decrackle(c, sr, s))
                });
            }
        });
    }

    fn fx_deemphasis(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "deemphasis", "De-emphasis", |ui| {
            let label = match self.fx.deemph_curve {
                cathar::Emphasis::Fm50 => "FM 50 µs",
                cathar::Emphasis::Fm75 => "FM 75 µs",
                cathar::Emphasis::CdIec => "CD / IEC",
            };
            egui::ComboBox::from_label("curve").selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.fx.deemph_curve, cathar::Emphasis::Fm50, "FM 50 µs");
                ui.selectable_value(&mut self.fx.deemph_curve, cathar::Emphasis::Fm75, "FM 75 µs");
                ui.selectable_value(&mut self.fx.deemph_curve, cathar::Emphasis::CdIec, "CD / IEC");
            });
            if apply_button(ui, "Apply de-emphasis").clicked() {
                let curve = self.fx.deemph_curve;
                self.apply_whole(ctx, "de-emphasis", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::deemphasis(c, sr, curve))
                });
            }
        });
    }

    fn fx_dewow(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "dewow", "Wow & flutter", |ui| {
            hint(ui, "Track a dominant tone's pitch drift and time-warp it flat.");
            if apply_button(ui, "Apply de-wow").clicked() {
                self.apply_whole(ctx, "de-wow", |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::dewow(c, sr))
                });
            }
        });
    }

    fn fx_inpaint(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "inpaint", "Inpaint (gap fill)", |ui| {
            if apply_button(ui, "Auto-fill mutes/dropouts").clicked() {
                self.apply_whole(ctx, "inpaint (auto)", |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::inpaint_auto(c, sr, 50.0))
                });
            }
            let has_sel = self.selection.is_some();
            if apply_button_enabled(ui, has_sel, "Reconstruct selection").clicked() {
                if let Some(sel) = self.selection {
                    let (t0, t1) = (sel.t0, sel.t1);
                    self.apply_whole(ctx, "inpaint selection", move |a| {
                        let sr = a.sample_rate;
                        let start = (t0 * sr as f32) as usize;
                        let len = ((t1 - t0) * sr as f32) as usize;
                        a.map_channels(|c| cathar::inpaint_gap(c, start, len, 3))
                    });
                }
            }
        });
    }

    fn fx_azimuth(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let stereo =
            self.history.get(self.hist_idx).map(|a| a.channels.len() >= 2).unwrap_or(false);
        fx_section(ui, "azimuth", "Azimuth (stereo skew)", |ui| {
            if apply_button_enabled(ui, stereo, "Correct L/R skew").clicked() {
                self.apply_whole(ctx, "azimuth", |a| {
                    if a.channels.len() >= 2 {
                        let (l, r) = cathar::azimuth_correct(
                            &a.channels[0],
                            &a.channels[1],
                            a.sample_rate,
                            5.0,
                        );
                        AudioData { sample_rate: a.sample_rate, channels: vec![l, r] }
                    } else {
                        a.clone()
                    }
                });
            }
        });
    }

    fn fx_hpss(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "hpss", "Separate (HPSS)", |ui| {
            param_usize(ui, "Kernel", &mut self.fx.hpss_kernel, 3..=41);
            let k = self.fx.hpss_kernel | 1;
            ui.horizontal(|ui| {
                if apply_button(ui, "Keep harmonic").clicked() {
                    self.apply_whole(ctx, "HPSS harmonic", move |a| {
                        let sr = a.sample_rate;
                        a.map_channels(|c| cathar::hpss(c, sr, k).0)
                    });
                }
                if apply_button(ui, "Keep percussive").clicked() {
                    self.apply_whole(ctx, "HPSS percussive", move |a| {
                        let sr = a.sample_rate;
                        a.map_channels(|c| cathar::hpss(c, sr, k).1)
                    });
                }
            });
        });
    }

    fn fx_sms(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "sms", "Tonal purify (SMS)", |ui| {
            hint(ui, "Keep tracked sinusoidal partials, drop the noisy residual.");
            if apply_button(ui, "Apply SMS").clicked() {
                self.apply_whole(ctx, "SMS", |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::synthesize_sms(&cathar::analyze_sms(c, sr)))
                });
            }
        });
    }

    fn fx_transform(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        fx_section(ui, "transform", "Tempo / pitch / speed", |ui| {
            param_f32(ui, "Tempo", &mut self.fx.tempo_factor, 0.5..=2.0, 2);
            if apply_button(ui, "Apply tempo").clicked() {
                let f = self.fx.tempo_factor.max(0.01);
                self.apply_whole(ctx, "tempo", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| {
                        cathar::time_stretch(c, sr, 1.0 / f, cathar::StretchMode::Wsola)
                    })
                });
            }
            ui.add_space(8.0);
            param_f32(ui, "Pitch", &mut self.fx.pitch_semitones, -12.0..=12.0, 1);
            if apply_button(ui, "Apply pitch").clicked() {
                let st = self.fx.pitch_semitones;
                self.apply_whole(ctx, "pitch", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::pitch_shift(c, sr, st, cathar::StretchMode::Wsola))
                });
            }
            ui.add_space(8.0);
            param_f32(ui, "Speed", &mut self.fx.speed_factor, 0.5..=2.0, 2);
            if apply_button(ui, "Apply speed").clicked() {
                let f = self.fx.speed_factor.max(0.01);
                self.apply_whole(ctx, "speed", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| {
                        cathar::resample(c, (sr as f32 * f).round().max(1.0) as u32, sr)
                    })
                });
            }
        });
    }

    fn central(&mut self, ctx: &egui::Context) {
        // Spectrogram well stays dark in both themes (Logic-style editor canvas).
        let well_bg = Color32::from_rgb(16, 16, 18);
        const WAVE_TOTAL: f32 = 100.0 + TIME_AXIS_H;

        egui::CentralPanel::default().frame(egui::Frame::none().fill(well_bg)).show(ctx, |ui| {
            let avail = ui.available_size();
            let view_w = avail.x.max(64.0);
            let spectro_h = (avail.y - WAVE_TOTAL).max(80.0);

            let axis_text = Color32::from_rgb(200, 200, 205);
            let wave_bg = well_bg;

            // ---- Spectrogram (zoom = larger virtual image inside a scroll area;
            // scrolling pans, so no bespoke UV/pan maths) ----
            egui::ScrollArea::both()
                .id_salt("spectro")
                .drag_to_scroll(false)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_size(egui::vec2(view_w + FREQ_AXIS_W, spectro_h));
                    let image_w = view_w * self.zoom_x;
                    let image_h = spectro_h * self.zoom_y;
                    let virt = egui::vec2(image_w + FREQ_AXIS_W, image_h + TIME_AXIS_H);
                    let (resp, painter) = ui.allocate_painter(virt, Sense::click_and_drag());
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
                        painter.rect_filled(image, 0.0, wave_bg);
                        if let Some(logo) = &self.logo {
                            // Centred logo splash, sized to the canvas.
                            let [lw, lh] = logo.size();
                            let aspect = lw as f32 / lh as f32;
                            let target_h = (image.height() * 0.4).min(240.0);
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
                                "Open an audio file  ·  ⌘O",
                                egui::FontId::proportional(13.0),
                                axis_text.gamma_multiply(0.55),
                            );
                        } else {
                            painter.text(
                                image.center(),
                                egui::Align2::CENTER_CENTER,
                                "Open an audio file",
                                egui::FontId::proportional(18.0),
                                axis_text.gamma_multiply(0.55),
                            );
                        }
                    }

                    if self.has_audio() && self.duration > 0.0 {
                        axes::draw_freq_axis(&painter, outer, image, self.nyquist(), axis_text);
                        axes::draw_time_axis(&painter, outer, image, self.duration, axis_text);
                        self.handle_spectro_interaction(&resp, image);
                        self.draw_selection(&painter, image);
                        self.draw_playhead(&painter, image);
                    }
                });

            // ---- Waveform (aligned with spectro time axis) ----
            let (wresp, wpainter) = ui.allocate_painter(
                egui::vec2(view_w + FREQ_AXIS_W, 100.0 + TIME_AXIS_H),
                Sense::click(),
            );
            let wouter = wresp.rect;
            let wimage = Rect::from_min_size(
                pos2(wouter.left() + FREQ_AXIS_W, wouter.top()),
                egui::vec2(view_w, 100.0),
            );
            wpainter.rect_filled(wimage, 0.0, wave_bg);
            self.draw_waveform(&wpainter, wimage);
            if self.duration > 0.0 {
                axes::draw_time_axis(&wpainter, wouter, wimage, self.duration, axis_text);
            }
            if wresp.clicked() {
                if let Some(p) = wresp.interact_pointer_pos() {
                    if wimage.contains(p) {
                        let t = ((p.x - wimage.left()) / wimage.width()).clamp(0.0, 1.0)
                            * self.duration;
                        if let Some(eng) = &self.engine {
                            eng.seek(t);
                        }
                    }
                }
            }
            self.draw_playhead(&wpainter, wimage);
        });
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
        // A plain click (no drag) seeks.
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
        painter.rect_filled(sel_rect, 0.0, Color32::from_rgba_unmultiplied(80, 160, 255, 40));
        painter.rect_stroke(sel_rect, 0.0, Stroke::new(1.5, Color32::from_rgb(120, 190, 255)));
    }

    fn draw_playhead(&self, painter: &egui::Painter, rect: Rect) {
        if self.duration <= 0.0 {
            return;
        }
        let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
        let x = rect.left() + (pos / self.duration).clamp(0.0, 1.0) * rect.width();
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, Color32::from_rgb(255, 240, 120)),
        );
    }

    fn draw_waveform(&self, painter: &egui::Painter, rect: Rect) {
        if self.waveform.is_empty() {
            return;
        }
        let mid = rect.center().y;
        let half = rect.height() * 0.5;
        let n = self.waveform.len();
        for (i, &(lo, hi)) in self.waveform.iter().enumerate() {
            let x = rect.left() + i as f32 / n as f32 * rect.width();
            painter.line_segment(
                [pos2(x, mid - hi * half), pos2(x, mid - lo * half)],
                Stroke::new(1.0, Color32::from_rgb(90, 170, 120)),
            );
        }
    }
}

/// Decode the bundled logo PNG into a texture for the empty-state splash.
///
/// `logo.png` has a solid dark background baked in; key it out to transparent
/// (sampling the corner colour) so the logo isn't a black box on the canvas.
fn load_logo(ctx: &egui::Context) -> Option<TextureHandle> {
    let bytes = include_bytes!("../../../docs/logo.png");
    let mut img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    let bg = *img.get_pixel(0, 0);
    let near = |a: u8, b: u8| (a as i32 - b as i32).abs() < 44;
    for px in img.pixels_mut() {
        if near(px[0], bg[0]) && near(px[1], bg[1]) && near(px[2], bg[2]) {
            px[3] = 0; // transparent background
        }
    }
    let color = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture("logo", color, TextureOptions::LINEAR))
}

/// Display hop for a signal of `len` samples: the default fine hop, widened
/// only enough that the spectrogram never exceeds `MAX_COLS` columns (so the
/// GPU texture stays within the 16384 side limit).
fn display_hop(len: usize) -> usize {
    if len <= FFT_SIZE {
        return HOP;
    }
    (len - FFT_SIZE).div_ceil(MAX_COLS - 1).max(HOP)
}

fn fmt_time(secs: f32) -> String {
    let secs = secs.max(0.0);
    let m = (secs / 60.0).floor() as u32;
    let s = secs - (m * 60) as f32;
    format!("{m}:{s:04.1}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame count (spectrogram width) must stay within the GPU texture limit
    /// for any signal length — this is the regression the 19665-col panic hit.
    #[test]
    fn display_hop_bounds_texture_width() {
        // Short files keep the fine hop.
        assert_eq!(display_hop(1000), HOP);
        assert_eq!(display_hop(FFT_SIZE), HOP);
        // Long files (here ~30 min at 48 kHz) widen the hop so width ≤ MAX_COLS.
        for &len in &[1_000_000usize, 10_070_016, 48_000 * 1800] {
            let hop = display_hop(len);
            let frames = (len - FFT_SIZE) / hop + 1;
            assert!(frames <= MAX_COLS, "len {len}: {frames} cols > {MAX_COLS}");
        }
    }
}
