//! Optional terminal spectrogram viewer — a lightweight nod to RX's spectral
//! display. Renders a `cathar::Spectrogram` as a truecolor heatmap using unicode
//! half-blocks (two frequency bins per text row), with a movable crosshair that
//! reads out time / frequency / level. Built only with `--features tui`.

use anyhow::{Result, bail};
use cathar::{Spectrogram, spectrogram};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::prelude::*;
use ratatui::style::Color;
use std::time::Duration;

/// Decode `input`, compute its spectrogram, and run the interactive viewer.
pub(crate) fn run(input: &str, fft_size: usize, hop: usize) -> Result<()> {
    let audio = cathar::AudioData::from_file(input).map_err(|e| anyhow::anyhow!("{e}"))?;
    let n = audio.channels.iter().map(Vec::len).max().unwrap_or(0);
    if n == 0 {
        bail!("no audio samples in {input}");
    }
    // Mix down to mono for the analysis view.
    let mut mono = vec![0.0f32; n];
    for ch in &audio.channels {
        for (i, &s) in ch.iter().enumerate() {
            mono[i] += s;
        }
    }
    let nch = audio.channels.len().max(1) as f32;
    for s in &mut mono {
        *s /= nch;
    }

    let spec = spectrogram(&mono, audio.sample_rate, fft_size, hop);
    if spec.frames() == 0 {
        bail!("signal too short for a spectrogram (need ≥ {fft_size} samples)");
    }

    let mut app = App::new(input.to_string(), spec, n as f32 / audio.sample_rate as f32);
    let mut term = ratatui::init();
    let res = app.run(&mut term);
    ratatui::restore();
    res
}

struct App {
    title: String,
    spec: Spectrogram,
    duration: f32,
    db_lo: f32,
    db_hi: f32,
    x0: usize,             // first frame in view
    frames_per_col: usize, // 0 = auto-fit
    cur_fpc: usize,        // effective frames-per-col from the last draw
    last_hw: u16,          // heatmap width from the last draw
    log_freq: bool,
    cx: u16, // crosshair column within the heatmap
    cy: u16, // crosshair row within the heatmap
    truecolor: bool,
    quit: bool,
}

impl App {
    fn new(title: String, spec: Spectrogram, duration: f32) -> Self {
        // Color range: 80 dB below the loudest bin gives good contrast.
        let hi = spec.data.iter().copied().fold(f32::MIN, f32::max);
        Self {
            title,
            spec,
            duration,
            db_hi: hi,
            db_lo: hi - 80.0,
            x0: 0,
            frames_per_col: 0,
            cur_fpc: 1,
            last_hw: 1,
            log_freq: false,
            cx: 0,
            cy: 0,
            truecolor: crate::termcolor::supports_truecolor(),
            quit: false,
        }
    }

    fn run(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        while !self.quit {
            term.draw(|f| self.draw(f))?;
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(k) = event::read()? {
                    if k.kind == KeyEventKind::Press {
                        self.on_key(k.code);
                    }
                }
            }
        }
        Ok(())
    }

    fn on_key(&mut self, code: KeyCode) {
        let frames = self.spec.frames();
        let span = self.cur_fpc * self.last_hw as usize; // frames currently shown
        let step = (span / 6).max(1);
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('f') => self.log_freq = !self.log_freq,
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.frames_per_col = (self.cur_fpc / 2).max(1);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.frames_per_col = (self.cur_fpc * 2).min(frames.max(1));
            }
            KeyCode::Char('0') => {
                self.frames_per_col = 0;
                self.x0 = 0;
            }
            KeyCode::Left => {
                if self.cx > 0 {
                    self.cx -= 1;
                } else {
                    self.x0 = self.x0.saturating_sub(step);
                }
            }
            KeyCode::Right => {
                if self.cx + 1 < self.last_hw {
                    self.cx += 1;
                } else {
                    self.x0 = (self.x0 + step).min(frames.saturating_sub(span));
                }
            }
            KeyCode::Up => self.cy = self.cy.saturating_sub(1),
            KeyCode::Down => self.cy += 1,
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        if area.width < 12 || area.height < 6 {
            return;
        }
        const GUTTER: u16 = 8; // left freq-axis labels
        let hx = area.x + GUTTER;
        let hy = area.y + 1; // below title
        let hw = area.width - GUTTER;
        let hh = area.height.saturating_sub(4); // title + time-axis + help (+1)
        self.last_hw = hw;

        let frames = self.spec.frames();
        let bins = self.spec.bins;
        let fpc = if self.frames_per_col == 0 {
            frames.div_ceil(hw as usize).max(1)
        } else {
            self.frames_per_col
        };
        self.cur_fpc = fpc;
        if self.cx >= hw {
            self.cx = hw - 1;
        }
        if self.cy >= hh {
            self.cy = hh.saturating_sub(1);
        }

        let buf = frame.buffer_mut();
        let subrows = hh as usize * 2;

        // ── heatmap ──
        for col in 0..hw {
            let f_lo = self.x0 + col as usize * fpc;
            if f_lo >= frames {
                break;
            }
            let f_hi = (f_lo + fpc).min(frames);
            for trow in 0..hh {
                let top = self.cell_db(f_lo, f_hi, trow as usize * 2, subrows, bins);
                let bot = self.cell_db(f_lo, f_hi, trow as usize * 2 + 1, subrows, bins);
                if let Some(cell) = buf.cell_mut((hx + col, hy + trow)) {
                    cell.set_symbol("▀");
                    cell.set_fg(magma(self.norm(top), self.truecolor));
                    cell.set_bg(magma(self.norm(bot), self.truecolor));
                }
            }
        }

        // ── crosshair ──
        let chx = hx + self.cx;
        let chy = hy + self.cy;
        for r in 0..hh {
            if let Some(cell) = buf.cell_mut((chx, hy + r)) {
                cell.set_fg(Color::White);
                cell.set_symbol("│");
            }
        }
        if let Some(cell) = buf.cell_mut((chx, chy)) {
            cell.set_symbol("┼");
            cell.set_fg(Color::White);
        }

        // ── freq-axis labels (left gutter) ──
        for trow in (0..hh).step_by(3) {
            let bin = self.subrow_bin(trow as usize * 2, subrows, bins);
            let khz = self.spec.bin_hz(bin) / 1000.0;
            put_str(buf, area.x, hy + trow, &format!("{khz:>6.1}k"), Color::DarkGray, Color::Reset);
        }

        // ── title ──
        let title = format!(
            " cathar view  {}  ·  {:.1}s  ·  {} Hz  ·  fft {} ",
            self.title, self.duration, self.spec.sample_rate, self.spec.fft_size
        );
        put_str(buf, area.x, area.y, &fit(&title, area.width), Color::Black, Color::Cyan);

        // ── time axis ──
        let ty = hy + hh;
        let t_lo = self.spec.frame_time(self.x0.min(frames.saturating_sub(1)));
        let last_f = (self.x0 + fpc * hw as usize).min(frames.saturating_sub(1));
        let t_hi = self.spec.frame_time(last_f);
        put_str(buf, hx, ty, &format!("{t_lo:.2}s"), Color::DarkGray, Color::Reset);
        let rlabel = format!("{t_hi:.2}s");
        let rx = area.x + area.width.saturating_sub(rlabel.len() as u16);
        put_str(buf, rx, ty, &rlabel, Color::DarkGray, Color::Reset);

        // ── footer: crosshair readout + help ──
        let cur_f = (self.x0 + self.cx as usize * fpc).min(frames.saturating_sub(1));
        let cur_bin = self.subrow_bin(self.cy as usize * 2, subrows, bins);
        let readout = format!(
            " t={:.2}s  f={:.0}Hz  {:.0}dB │ [+/-] zoom  [←→↑↓] move  [f] {}  [0] reset  [q] quit ",
            self.spec.frame_time(cur_f),
            self.spec.bin_hz(cur_bin),
            self.spec.get(cur_f, cur_bin),
            if self.log_freq { "linear" } else { "log" },
        );
        put_str(
            buf,
            area.x,
            area.y + area.height - 1,
            &fit(&readout, area.width),
            Color::White,
            Color::Indexed(236),
        );
    }

    /// Max-pooled dB over a frame range and the bin span of one sub-row.
    fn cell_db(&self, f_lo: usize, f_hi: usize, subrow: usize, subrows: usize, bins: usize) -> f32 {
        let b0 = self.subrow_bin(subrow + 1, subrows, bins);
        let b1 = self.subrow_bin(subrow, subrows, bins);
        let (lo, hi) = (b0.min(b1), b0.max(b1));
        let mut peak = f32::MIN;
        for f in f_lo..f_hi {
            for b in lo..=hi {
                peak = peak.max(self.spec.get(f, b));
            }
        }
        peak
    }

    /// Frequency bin for a sub-row (0 = top = Nyquist, `subrows-1` = bottom = DC).
    fn subrow_bin(&self, subrow: usize, subrows: usize, bins: usize) -> usize {
        let frac = 1.0 - subrow as f32 / (subrows.max(2) - 1) as f32; // 1=Nyquist, 0=DC
        let frac = frac.clamp(0.0, 1.0);
        if self.log_freq {
            let max_hz = self.spec.bin_hz(bins - 1).max(1.0);
            let min_hz = 20.0_f32.min(max_hz * 0.5);
            let hz = min_hz * (max_hz / min_hz).powf(frac);
            ((hz / max_hz * (bins - 1) as f32).round() as usize).min(bins - 1)
        } else {
            ((frac * (bins - 1) as f32).round() as usize).min(bins - 1)
        }
    }

    fn norm(&self, db: f32) -> f32 {
        ((db - self.db_lo) / (self.db_hi - self.db_lo)).clamp(0.0, 1.0)
    }
}

/// Magma-ish colormap: `t` in `[0,1]` → a terminal color (truecolor or xterm-256).
fn magma(t: f32, truecolor: bool) -> Color {
    const STOPS: [(f32, (u8, u8, u8)); 5] = [
        (0.00, (0, 0, 4)),
        (0.25, (43, 17, 86)),
        (0.50, (114, 31, 107)),
        (0.75, (216, 71, 68)),
        (1.00, (252, 253, 191)),
    ];
    let t = t.clamp(0.0, 1.0);
    for w in STOPS.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let f = ((t - t0) / (t1 - t0)).clamp(0.0, 1.0);
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
            return crate::termcolor::rgb(
                lerp(c0.0, c1.0),
                lerp(c0.1, c1.1),
                lerp(c0.2, c1.2),
                truecolor,
            );
        }
    }
    crate::termcolor::rgb(252, 253, 191, truecolor)
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

/// Pad/truncate a string to exactly `width` cells.
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn demo_app() -> App {
        let fs = 44_100;
        let sig: Vec<f32> = (0..fs)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / fs as f32).sin())
            .collect();
        App::new("test.wav".into(), spectrogram(&sig, fs, 2048, 512), 1.0)
    }

    #[test]
    fn renders_without_panic() {
        let mut app = demo_app();
        // Normal size and a tiny size (hits the early-return guard).
        Terminal::new(TestBackend::new(100, 30)).unwrap().draw(|f| app.draw(f)).unwrap();
        Terminal::new(TestBackend::new(8, 4)).unwrap().draw(|f| app.draw(f)).unwrap();
    }

    #[test]
    fn keys_move_zoom_and_quit() {
        let mut app = demo_app();
        let mut term = Terminal::new(TestBackend::new(100, 30)).unwrap();
        term.draw(|f| app.draw(f)).unwrap();
        for code in [KeyCode::Right, KeyCode::Down, KeyCode::Char('+'), KeyCode::Char('f')] {
            app.on_key(code);
            term.draw(|f| app.draw(f)).unwrap();
        }
        app.on_key(KeyCode::Char('q'));
        assert!(app.quit);
    }
}
