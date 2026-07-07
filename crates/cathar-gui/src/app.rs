//! The Cathar spectral editor application.

use crate::engine::Engine;
use crate::spectral_edit::{Selection, SpectralOp, apply_spectral};
use crate::spectro::{colorize, compute_spectrogram, mono_mix, waveform_envelope};
use cathar::{AudioData, Denoiser, SpectralDenoiser, Spectrogram};
use egui::{Color32, Pos2, Rect, Sense, Stroke, TextureHandle, TextureOptions, pos2};

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
    dequant_bits: u32,
    dequant_strength: f32,
    normalize_lufs: f32,
    normalize_ceiling: f32,
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
            dequant_bits: 16,
            dequant_strength: 0.7,
            normalize_lufs: -14.0,
            normalize_ceiling: -1.0,
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
    status: String,
}

impl CatharGui {
    /// Build the app; opens the audio device if one is available.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Swap egui's bundled Ubuntu-Light for the host OS system font, and
        // apply a macOS-dark-flavoured theme.
        crate::fonts::install_system_font(&cc.egui_ctx);
        crate::theme::apply(&cc.egui_ctx);
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
            status,
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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("📂 Open").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Audio", &["wav", "mp3", "flac", "ogg", "m4a", "aiff", "aif"])
                        .pick_file()
                    {
                        self.open(ctx, p.display().to_string());
                    }
                }
                if ui.add_enabled(self.has_audio(), egui::Button::new("💾 Save")).clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Audio", &["wav", "flac", "aiff"])
                        .set_file_name("edited.wav")
                        .save_file()
                    {
                        self.save(p.display().to_string());
                    }
                }
                ui.separator();

                let playing = self.engine.as_ref().map(|e| !e.is_paused()).unwrap_or(false);
                let label = if playing { "⏸ Pause" } else { "▶ Play" };
                if ui.add_enabled(self.has_audio(), egui::Button::new(label)).clicked() {
                    if let Some(eng) = &self.engine {
                        eng.toggle();
                    }
                }
                let pos = self.engine.as_ref().map(|e| e.pos()).unwrap_or(0.0);
                ui.label(format!("{}  /  {}", fmt_time(pos), fmt_time(self.duration)));

                ui.separator();
                if ui.add_enabled(self.hist_idx > 0, egui::Button::new("↶ Undo")).clicked() {
                    self.undo(ctx);
                }
                let can_redo = self.hist_idx + 1 < self.history.len();
                if ui.add_enabled(can_redo, egui::Button::new("↷ Redo")).clicked() {
                    self.redo(ctx);
                }
                ui.separator();
                let ab_enabled = self.original.is_some();
                let mut ab = self.show_original;
                if ui
                    .add_enabled(ab_enabled, egui::SelectableLabel::new(ab, "A/B: original"))
                    .clicked()
                {
                    ab = !ab;
                    self.show_original = ab;
                    self.recompute(ctx);
                }
            });
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
        egui::SidePanel::right("tools").min_width(230.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let enabled = self.has_audio();
                ui.heading("Restoration");
                ui.add_enabled_ui(enabled, |ui| {
                    self.fx_denoise(ui, ctx);
                    self.fx_dehum(ui, ctx);
                    self.fx_declick(ui, ctx);
                    self.fx_declip(ui, ctx);
                    self.fx_deess(ui, ctx);
                    self.fx_dereverb(ui, ctx);
                    self.fx_dequantize(ui, ctx);
                    self.fx_riaa_normalize(ui, ctx);
                });

                ui.separator();
                ui.heading("Selection");
                let has_sel = self.selection.is_some();
                ui.add_enabled_ui(enabled && has_sel, |ui| {
                    ui.add(egui::Slider::new(&mut self.gain_db, -60.0..=24.0).text("gain dB"));
                    if ui.button("Apply gain").clicked() {
                        let g = 10f32.powf(self.gain_db / 20.0);
                        self.apply_selection(ctx, SpectralOp::Gain(g), "Gain");
                    }
                    if ui.button("Heal (interpolate)").clicked() {
                        self.apply_selection(ctx, SpectralOp::Heal, "Heal");
                    }
                });
                if ui.add_enabled(has_sel, egui::Button::new("Clear selection")).clicked() {
                    self.selection = None;
                }

                ui.separator();
                ui.heading("Display");
                let r1 =
                    ui.add(egui::Slider::new(&mut self.db_floor, -120.0..=-30.0).text("dB floor"));
                let r2 =
                    ui.add(egui::Slider::new(&mut self.db_ceil, -30.0..=6.0).text("dB ceiling"));
                if r1.changed() || r2.changed() {
                    self.recolor(ctx);
                }

                ui.separator();
                ui.heading("View");
                ui.add(egui::Slider::new(&mut self.zoom_x, 1.0..=12.0).text("zoom ×time"));
                ui.add(egui::Slider::new(&mut self.zoom_y, 1.0..=8.0).text("zoom ×freq"));
                if ui.button("Reset view").clicked() {
                    self.zoom_x = 1.0;
                    self.zoom_y = 1.0;
                }

                ui.separator();
                ui.small(
                    "Drag on the spectrogram to select · scroll to pan when zoomed · \
                     click the waveform to seek.",
                );
            });
        });
    }

    fn fx_denoise(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("Denoise").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.fx.denoise_alpha, 1.0..=6.0).text("strength α"));
            ui.add(egui::Slider::new(&mut self.fx.denoise_beta, 0.0..=0.1).text("floor β"));
            if ui.button("Apply denoise").clicked() {
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
        egui::CollapsingHeader::new("De-hum").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.fx.hum_freq, 40.0..=120.0).text("base Hz"));
            ui.add(
                egui::Slider::new(&mut self.fx.hum_harmonics, 1..=10).text("harmonics").integer(),
            );
            if ui.button("Apply de-hum").clicked() {
                let (f, h) = (self.fx.hum_freq, self.fx.hum_harmonics);
                self.apply_whole(ctx, "de-hum", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::dehum(c, sr, f, h))
                });
            }
        });
    }

    fn fx_declick(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("De-click").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.fx.declick_threshold, 1.0..=12.0).text("threshold"));
            ui.add(
                egui::Slider::new(&mut self.fx.declick_window, 8..=256).text("window").integer(),
            );
            if ui.button("Apply de-click").clicked() {
                let (t, w) = (self.fx.declick_threshold, self.fx.declick_window);
                self.apply_whole(ctx, "de-click", move |a| {
                    a.map_channels(|c| cathar::declick(c, t, w))
                });
            }
        });
    }

    fn fx_declip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("De-clip").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.fx.declip_threshold, 0.5..=1.0).text("clip level"));
            if ui.button("Apply de-clip").clicked() {
                let t = self.fx.declip_threshold;
                self.apply_whole(ctx, "de-clip", move |a| a.map_channels(|c| cathar::declip(c, t)));
            }
        });
    }

    fn fx_deess(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("De-ess").show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut self.fx.deess_crossover, 3000.0..=12000.0)
                    .text("crossover Hz"),
            );
            ui.add(
                egui::Slider::new(&mut self.fx.deess_threshold_db, -60.0..=0.0)
                    .text("threshold dB"),
            );
            ui.add(egui::Slider::new(&mut self.fx.deess_ratio, 1.0..=10.0).text("ratio"));
            if ui.button("Apply de-ess").clicked() {
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
        egui::CollapsingHeader::new("De-reverb").show(ui, |ui| {
            ui.add(egui::Slider::new(&mut self.fx.dereverb_strength, 0.0..=1.0).text("strength"));
            if ui.button("Apply de-reverb").clicked() {
                let s = self.fx.dereverb_strength;
                self.apply_whole(ctx, "de-reverb", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::dereverb(c, sr, s))
                });
            }
        });
    }

    fn fx_dequantize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("Dequantize").show(ui, |ui| {
            ui.add(
                egui::Slider::new(&mut self.fx.dequant_bits, 4..=24).text("source bits").integer(),
            );
            ui.add(egui::Slider::new(&mut self.fx.dequant_strength, 0.0..=1.0).text("strength"));
            if ui.button("Apply dequantize").clicked() {
                let (b, s) = (self.fx.dequant_bits, self.fx.dequant_strength);
                self.apply_whole(ctx, "dequantize", move |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::dequantize(c, sr, b, s))
                });
            }
        });
    }

    fn fx_riaa_normalize(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::CollapsingHeader::new("RIAA / Normalize").show(ui, |ui| {
            if ui.button("RIAA de-emphasis").clicked() {
                self.apply_whole(ctx, "RIAA", |a| {
                    let sr = a.sample_rate;
                    a.map_channels(|c| cathar::riaa_deemphasis(c, sr))
                });
            }
            ui.separator();
            ui.add(
                egui::Slider::new(&mut self.fx.normalize_lufs, -30.0..=-6.0).text("target LUFS"),
            );
            ui.add(
                egui::Slider::new(&mut self.fx.normalize_ceiling, -6.0..=0.0).text("ceiling dBTP"),
            );
            if ui.button("Normalize").clicked() {
                let (l, c) = (self.fx.normalize_lufs, self.fx.normalize_ceiling);
                self.apply_whole(ctx, "normalize", move |a| a.normalize_r128(l, c));
            }
        });
    }

    fn central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            let avail = ui.available_size();
            let spectro_h = (avail.y - 120.0).max(120.0);
            let view_w = avail.x.max(64.0);

            // ---- Spectrogram (zoom = larger virtual image inside a scroll area;
            // scrolling pans, so no bespoke UV/pan maths) ----
            egui::ScrollArea::both()
                .drag_to_scroll(false)
                .max_height(spectro_h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let virt = egui::vec2(view_w * self.zoom_x, spectro_h * self.zoom_y);
                    let (resp, painter) = ui.allocate_painter(virt, Sense::click_and_drag());
                    let rect = resp.rect;
                    if let Some(tex) = &self.texture {
                        painter.image(
                            tex.id(),
                            rect,
                            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    } else {
                        painter.rect_filled(rect, 0.0, Color32::from_gray(20));
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "Open an audio file",
                            egui::FontId::proportional(18.0),
                            Color32::GRAY,
                        );
                    }

                    if self.has_audio() && self.duration > 0.0 {
                        self.handle_spectro_interaction(&resp, rect);
                        self.draw_selection(&painter, rect);
                        self.draw_playhead(&painter, rect);
                    }
                });

            // ---- Waveform (full width, unzoomed) ----
            let (wresp, wpainter) = ui.allocate_painter(egui::vec2(view_w, 100.0), Sense::click());
            let wrect = wresp.rect;
            wpainter.rect_filled(wrect, 0.0, Color32::from_gray(16));
            self.draw_waveform(&wpainter, wrect);
            if wresp.clicked() {
                if let Some(p) = wresp.interact_pointer_pos() {
                    let t = ((p.x - wrect.left()) / wrect.width()).clamp(0.0, 1.0) * self.duration;
                    if let Some(eng) = &self.engine {
                        eng.seek(t);
                    }
                }
            }
            self.draw_playhead(&wpainter, wrect);
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
