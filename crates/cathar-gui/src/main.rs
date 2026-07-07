//! Cathar GUI — an all-Rust, iZotope-RX-style spectral editor front-end for the
//! `cathar` restoration library. Open a file, see its spectrogram, draw a
//! time-frequency selection and attenuate/boost/heal it, or run any whole-file
//! restoration stage, then play back and save.

mod app;
mod colormap;
mod engine;
mod fonts;
mod spectral_edit;
mod spectro;
mod theme;

use app::CatharGui;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([720.0, 480.0])
            .with_title("Cathar — spectral editor"),
        ..Default::default()
    };
    eframe::run_native("cathar-gui", options, Box::new(|cc| Ok(Box::new(CatharGui::new(cc)))))
}
