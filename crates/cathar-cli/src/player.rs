//! `cathar play` — a Winamp/cava-style terminal player: plays the file through
//! the system audio device (rodio) and animates a live spectrum analyzer (with
//! peak-hold caps) or an oscilloscope, synced to playback. Built with
//! `--features tui`.

use anyhow::{Result, anyhow, bail};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::style::Color;
use std::num::NonZero;
use std::time::Duration;

/// Decode `input`, start playback, and run the live visualizer.
pub(crate) fn run(input: &str, fft_size: usize) -> Result<()> {
    let audio = cathar::AudioData::from_file(input).map_err(|e| anyhow!("{e}"))?;
    let sr = audio.sample_rate;
    let nch = audio.channels.len().max(1);
    let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        bail!("no audio samples in {input}");
    }
    let duration = n as f32 / sr as f32;

    // Interleave for playback; mono mixdown for analysis.
    let mut interleaved = vec![0.0f32; n * nch];
    let mut mono = vec![0.0f32; n];
    for (c, ch) in audio.channels.iter().enumerate() {
        for (i, &s) in ch.iter().enumerate() {
            interleaved[i * nch + c] = s;
            mono[i] += s;
        }
    }
    for s in &mut mono {
        *s /= nch as f32;
    }

    // Audio output (system device via rodio).
    let stream = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|e| anyhow!("no audio output device: {e}"))?;
    let player = rodio::Player::connect_new(stream.mixer());
    let ch = NonZero::new(nch as u16).ok_or_else(|| anyhow!("zero channels"))?;
    let rate = NonZero::new(sr).ok_or_else(|| anyhow!("zero sample rate"))?;
    player.append(rodio::buffer::SamplesBuffer::new(ch, rate, interleaved));

    let mut ui = Ui::new(short_name(input), duration, sr, fft_size, mono);
    let mut term = ratatui::init();
    let res = ui.run(&mut term, &player);
    player.stop();
    ratatui::restore();
    res
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Spectrum,
    Scope,
}

struct Ui {
    title: String,
    duration: f32,
    sample_rate: u32,
    fft_size: usize,
    mono: Vec<f32>,
    levels: Vec<f32>, // smoothed bar heights, 0..1
    peaks: Vec<f32>,  // peak-hold caps, 0..1
    mode: Mode,
    quit: bool,
}

impl Ui {
    fn new(
        title: String,
        duration: f32,
        sample_rate: u32,
        fft_size: usize,
        mono: Vec<f32>,
    ) -> Self {
        Self {
            title,
            duration,
            sample_rate,
            fft_size,
            mono,
            levels: Vec::new(),
            peaks: Vec::new(),
            mode: Mode::Spectrum,
            quit: false,
        }
    }

    fn run(&mut self, term: &mut DefaultTerminal, player: &rodio::Player) -> Result<()> {
        while !self.quit {
            let pos = player.get_pos().as_secs_f32();
            if pos >= self.duration + 0.2 {
                break; // track finished
            }
            let paused = player.is_paused();
            term.draw(|f| self.draw(f, pos, paused))?;

            // ~30 fps; drain any pending keys.
            if event::poll(Duration::from_millis(33))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        self.on_key(k.code, player, pos);
                    }
                }
            }
        }
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode, player: &rodio::Player, pos: f32) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('m') => {
                self.mode = match self.mode {
                    Mode::Spectrum => Mode::Scope,
                    Mode::Scope => Mode::Spectrum,
                };
            }
            KeyCode::Char(' ') => {
                if player.is_paused() {
                    player.play();
                } else {
                    player.pause();
                }
            }
            KeyCode::Left => {
                let _ = player.try_seek(Duration::from_secs_f32((pos - 5.0).max(0.0)));
            }
            KeyCode::Right => {
                let t = (pos + 5.0).min(self.duration);
                let _ = player.try_seek(Duration::from_secs_f32(t));
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame, pos: f32, paused: bool) {
        let area = frame.area();
        if area.width < 8 || area.height < 5 {
            return;
        }
        // title (row 0), viz (1..h-2), progress (h-2), help (h-1)
        let vy = area.y + 1;
        let vh = area.height.saturating_sub(3);
        let vw = area.width;

        // The audio window at the current playback position.
        let idx = ((pos * self.sample_rate as f32) as usize).min(self.mono.len());
        let mut window = vec![0.0f32; self.fft_size];
        let avail = self.mono.len().saturating_sub(idx).min(self.fft_size);
        window[..avail].copy_from_slice(&self.mono[idx..idx + avail]);

        let viz = Rect { x: area.x, y: vy, width: vw, height: vh };
        let buf = frame.buffer_mut();
        match self.mode {
            Mode::Spectrum => self.draw_spectrum(buf, viz, &window, paused),
            Mode::Scope => draw_scope(buf, viz, &window),
        }

        // ── title ──
        let title = format!(
            " ♪ cathar play  {}  ·  {} Hz {}",
            self.title,
            self.sample_rate,
            if paused { " · ⏸ PAUSED" } else { "" }
        );
        put_str(buf, area.x, area.y, &fit(&title, area.width), Color::Black, Color::Magenta);

        // ── progress bar ──
        let py = vy + vh;
        let frac = (pos / self.duration.max(0.001)).clamp(0.0, 1.0);
        let filled = (frac * area.width as f32) as u16;
        let bar: String = (0..area.width).map(|x| if x < filled { '━' } else { '─' }).collect();
        put_str(buf, area.x, py, &bar, Color::Magenta, Color::Reset);
        let tlabel = format!(" {}/{} ", mmss(pos), mmss(self.duration));
        put_str(buf, area.x + 1, py, &tlabel, Color::White, Color::Reset);

        // ── help ──
        let help = " [space] pause  [←→] seek  [m] spectrum/scope  [q] quit ";
        put_str(
            buf,
            area.x,
            area.y + area.height - 1,
            &fit(help, area.width),
            Color::Gray,
            Color::Indexed(236),
        );
    }

    fn draw_spectrum(&mut self, buf: &mut Buffer, area: Rect, window: &[f32], paused: bool) {
        let (x0, y0, w, h) = (area.x, area.y, area.width, area.height);
        let bands = w as usize;
        if self.levels.len() != bands {
            self.levels = vec![0.0; bands];
            self.peaks = vec![0.0; bands];
        }
        let spec = cathar::spectrogram(window, self.sample_rate, self.fft_size, self.fft_size);
        let bins = spec.bins;
        let bin_hz = self.sample_rate as f32 / self.fft_size as f32;
        let f_min = 30.0f32;
        let f_max = (self.sample_rate as f32 * 0.5).min(16_000.0).max(f_min * 2.0);
        const FLOOR: f32 = -70.0;
        const CEIL: f32 = -5.0;

        for b in 0..bands {
            // Log-spaced frequency band → bin range → peak dB.
            let frac0 = b as f32 / bands as f32;
            let frac1 = (b + 1) as f32 / bands as f32;
            let lo = ((f_min * (f_max / f_min).powf(frac0)) / bin_hz) as usize;
            let hi = (((f_min * (f_max / f_min).powf(frac1)) / bin_hz) as usize).max(lo);
            let mut db = FLOOR;
            if spec.frames() > 0 {
                for bin in lo..=hi.min(bins - 1) {
                    db = db.max(spec.get(0, bin));
                }
            }
            let target = ((db - FLOOR) / (CEIL - FLOOR)).clamp(0.0, 1.0);
            // Fast attack, gravity decay (frozen while paused).
            if paused {
                // hold
            } else if target >= self.levels[b] {
                self.levels[b] = target;
            } else {
                self.levels[b] = (self.levels[b] * 0.80).max(target);
            }
            if self.levels[b] >= self.peaks[b] {
                self.peaks[b] = self.levels[b];
            } else if !paused {
                self.peaks[b] = (self.peaks[b] - 0.012).max(self.levels[b]);
            }
        }

        const BLOCKS: [&str; 9] = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"];
        let total = h as usize * 8;
        for b in 0..bands {
            let eighths = (self.levels[b] * total as f32).round() as usize;
            let peak_row = (self.peaks[b] * (h as f32 - 1.0)).round() as u16; // from bottom
            for r in 0..h {
                let from_bottom = h - 1 - r;
                let filled = eighths.saturating_sub(from_bottom as usize * 8).min(8);
                let fg = bar_color(from_bottom as f32 / (h as f32 - 1.0).max(1.0));
                if let Some(cell) = buf.cell_mut((x0 + b as u16, y0 + r)) {
                    cell.set_symbol(BLOCKS[filled]);
                    cell.set_fg(fg);
                    // Peak-hold cap, where the bar itself isn't already full.
                    if from_bottom == peak_row && filled < 8 {
                        cell.set_symbol("▔");
                        cell.set_fg(Color::White);
                    }
                }
            }
        }
    }
}

/// Bottom-up bar color: green → yellow → red with height fraction `t`.
fn bar_color(t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        let f = t / 0.5;
        Color::Rgb((80.0 + 175.0 * f) as u8, 255, 40)
    } else {
        let f = (t - 0.5) / 0.5;
        Color::Rgb(255, (255.0 - 215.0 * f) as u8, 40)
    }
}

/// Oscilloscope: the current window drawn as a centered waveform.
fn draw_scope(buf: &mut Buffer, area: Rect, window: &[f32]) {
    let (x0, y0, w, h) = (area.x, area.y, area.width, area.height);
    let mid = h / 2;
    for col in 0..w {
        let i = (col as usize * window.len() / w.max(1) as usize).min(window.len() - 1);
        let v = window[i].clamp(-1.0, 1.0);
        let off = (v * (h as f32 / 2.0)) as i32;
        let y = (mid as i32 - off).clamp(0, h as i32 - 1) as u16;
        if let Some(cell) = buf.cell_mut((x0 + col, y0 + y)) {
            cell.set_symbol("•");
            cell.set_fg(Color::Rgb(80, 220, 120));
        }
    }
}

fn put_str(buf: &mut Buffer, x: u16, y: u16, s: &str, fg: Color, bg: Color) {
    let mut cx = x;
    for ch in s.chars() {
        if let Some(cell) = buf.cell_mut((cx, y)) {
            cell.set_char(ch);
            cell.set_fg(fg);
            cell.set_bg(bg);
        }
        cx = cx.saturating_add(1);
    }
}

fn fit(s: &str, width: u16) -> String {
    let w = width as usize;
    let len = s.chars().count();
    if len >= w {
        s.chars().take(w).collect()
    } else {
        let mut out = String::from(s);
        out.extend(std::iter::repeat_n(' ', w - len));
        out
    }
}

fn mmss(secs: f32) -> String {
    let s = secs.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

fn short_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn demo_ui() -> Ui {
        let fs = 44_100;
        let mono: Vec<f32> = (0..fs * 2)
            .map(|i| 0.5 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / fs as f32).sin())
            .collect();
        Ui::new("song.wav".into(), 2.0, fs, 2048, mono)
    }

    #[test]
    fn spectrum_and_scope_render() {
        let mut ui = demo_ui();
        let mut term = Terminal::new(TestBackend::new(120, 30)).unwrap();
        term.draw(|f| ui.draw(f, 0.5, false)).unwrap();
        // bars should have risen off the floor for a loud tone
        assert!(ui.levels.iter().cloned().fold(0.0, f32::max) > 0.0);
        ui.mode = Mode::Scope;
        term.draw(|f| ui.draw(f, 0.5, false)).unwrap();
        // tiny terminal hits the early-return guard
        Terminal::new(TestBackend::new(6, 3)).unwrap().draw(|f| ui.draw(f, 0.0, false)).unwrap();
    }

    #[test]
    fn bar_color_spans_green_to_red() {
        assert!(matches!(bar_color(0.0), Color::Rgb(_, 255, _)));
        assert!(matches!(bar_color(1.0), Color::Rgb(255, _, _)));
    }
}
