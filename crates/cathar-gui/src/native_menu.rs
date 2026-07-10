//! OS-native application menus via [`muda`] (macOS menu bar, Windows window menu).
//!
//! Keeps File / Edit / View out of the egui client area so we don't double up
//! with the system chrome.

use muda::accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use raw_window_handle::HasWindowHandle;

/// Menu action identifiers (stable string ids for [`MenuEvent`]).
pub(crate) mod id {
    pub(crate) const OPEN: &str = "cathar.open";
    pub(crate) const OPEN_PLAYLIST: &str = "cathar.open_playlist";
    pub(crate) const IMPORT_M3U: &str = "cathar.import_m3u";
    pub(crate) const SAVE: &str = "cathar.save";
    pub(crate) const UNDO: &str = "cathar.undo";
    pub(crate) const REDO: &str = "cathar.redo";
    pub(crate) const THEME_SYSTEM: &str = "cathar.theme_system";
    pub(crate) const THEME_LIGHT: &str = "cathar.theme_light";
    pub(crate) const THEME_DARK: &str = "cathar.theme_dark";
    pub(crate) const RESET_ZOOM: &str = "cathar.reset_zoom";
    pub(crate) const VIEW_SPECTRO: &str = "cathar.view_spectro";
    pub(crate) const VIEW_PLAYLIST: &str = "cathar.view_playlist";
    pub(crate) const VIEW_VIZ: &str = "cathar.view_viz";
    pub(crate) const OPEN_EQ: &str = "cathar.open_eq";
}

/// Owns the native menu graph for the process lifetime.
pub(crate) struct NativeMenu {
    /// Root menu must stay alive for the OS to keep showing it.
    _menu: Menu,
    save: MenuItem,
    undo: MenuItem,
    redo: MenuItem,
    installed: bool,
}

impl NativeMenu {
    pub(crate) fn new() -> anyhow::Result<Self> {
        let menu = Menu::new();

        // macOS app menu (About / Hide / Quit) — left of File in the system bar.
        #[cfg(target_os = "macos")]
        {
            let app = Submenu::new("Cathar", true);
            app.append_items(&[
                &PredefinedMenuItem::about(Some("About Cathar"), None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ])?;
            menu.append(&app)?;
        }

        let open = MenuItem::with_id(
            id::OPEN,
            "Open…",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyO)),
        );
        let open_playlist = MenuItem::with_id(
            id::OPEN_PLAYLIST,
            "Add to Playlist…",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::SHIFT), Code::KeyO)),
        );
        let import_m3u = MenuItem::with_id(id::IMPORT_M3U, "Import M3U Playlist…", true, None);
        let save = MenuItem::with_id(
            id::SAVE,
            "Save…",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyS)),
        );

        #[cfg(target_os = "macos")]
        let file = Submenu::with_items("File", true, &[&open, &open_playlist, &import_m3u, &save])?;
        #[cfg(not(target_os = "macos"))]
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &open,
                &open_playlist,
                &import_m3u,
                &save,
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        )?;
        menu.append(&file)?;

        let undo = MenuItem::with_id(
            id::UNDO,
            "Undo",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyZ)),
        );
        let redo = MenuItem::with_id(
            id::REDO,
            "Redo",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::SHIFT), Code::KeyZ)),
        );
        let edit = Submenu::with_items("Edit", true, &[&undo, &redo])?;
        menu.append(&edit)?;

        // View-mode shortcuts: ⌘1 / ⌘2 / ⌘3 (macOS) · Ctrl+1… on Windows/Linux.
        let view_spectro = MenuItem::with_id(
            id::VIEW_SPECTRO,
            "Spectrogram",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit1)),
        );
        let view_playlist = MenuItem::with_id(
            id::VIEW_PLAYLIST,
            "Playlist Queue",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit2)),
        );
        let view_viz = MenuItem::with_id(
            id::VIEW_VIZ,
            "Visualizer",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit3)),
        );
        let open_eq = MenuItem::with_id(
            id::OPEN_EQ,
            "Equalizer…",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyE)),
        );
        // Themes: ⌘⌥1/2/3 — keep plain ⌘1–3 free for central viewer modes.
        let theme_system = MenuItem::with_id(
            id::THEME_SYSTEM,
            "Theme: System",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::ALT), Code::Digit1)),
        );
        let theme_light = MenuItem::with_id(
            id::THEME_LIGHT,
            "Theme: Light",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::ALT), Code::Digit2)),
        );
        let theme_dark = MenuItem::with_id(
            id::THEME_DARK,
            "Theme: Dark",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::ALT), Code::Digit3)),
        );
        // ⌘0 = “actual size” / reset zoom (Finder, browsers, DAWs).
        let reset_zoom = MenuItem::with_id(
            id::RESET_ZOOM,
            "Reset Spectrogram Zoom",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::Digit0)),
        );
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &view_spectro,
                &view_playlist,
                &view_viz,
                &PredefinedMenuItem::separator(),
                &open_eq,
                &PredefinedMenuItem::separator(),
                &theme_system,
                &theme_light,
                &theme_dark,
                &PredefinedMenuItem::separator(),
                &reset_zoom,
            ],
        )?;
        menu.append(&view)?;

        Ok(Self { _menu: menu, save, undo, redo, installed: false })
    }

    /// Attach the menu to the OS (once). Safe to call every frame until installed.
    pub(crate) fn ensure_installed(&mut self, window: &impl HasWindowHandle) {
        if self.installed {
            return;
        }

        #[cfg(target_os = "macos")]
        {
            let _ = window;
            // Re-assert product name immediately before installing the menu —
            // anything that touched NSApplication may have left the process
            // name as the binary (`cathar-gui` / crate id).
            crate::macos::set_process_name(crate::APP_NAME);
            self._menu.init_for_nsapp();
            crate::macos::force_app_menu_title(crate::APP_NAME);
            self.installed = true;
        }

        #[cfg(target_os = "windows")]
        {
            use raw_window_handle::RawWindowHandle;
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(h) = handle.as_raw() {
                    let hwnd = h.hwnd.get();
                    // SAFETY: hwnd comes from the live eframe window.
                    if unsafe { self._menu.init_for_hwnd(hwnd).is_ok() } {
                        self.installed = true;
                    }
                }
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = window;
            // Linux needs a GTK window handle from eframe (not exposed cleanly).
            // Keyboard shortcuts in the app still cover Open/Save/Undo/Redo.
            self.installed = true;
        }
    }

    pub(crate) fn set_enabled(&self, can_save: bool, can_undo: bool, can_redo: bool) {
        self.save.set_enabled(can_save);
        self.undo.set_enabled(can_undo);
        self.redo.set_enabled(can_redo);
    }
}

/// Drain pending menu events (non-blocking).
pub(crate) fn poll_events() -> Vec<String> {
    let mut out = Vec::new();
    let rx = MenuEvent::receiver();
    while let Ok(ev) = rx.try_recv() {
        out.push(ev.id.0.clone());
    }
    out
}
