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
    pub(crate) const CLEAR_SELECTION: &str = "cathar.clear_selection";
    pub(crate) const HEAL_SELECTION: &str = "cathar.heal_selection";
    pub(crate) const ATTENUATE_SELECTION: &str = "cathar.attenuate_selection";
    pub(crate) const COMPARE_ORIGINAL: &str = "cathar.compare_original";
    pub(crate) const THEME_SYSTEM: &str = "cathar.theme_system";
    pub(crate) const THEME_LIGHT: &str = "cathar.theme_light";
    pub(crate) const THEME_DARK: &str = "cathar.theme_dark";
    pub(crate) const RESET_ZOOM: &str = "cathar.reset_zoom";
    pub(crate) const VIEW_SPECTRO: &str = "cathar.view_spectro";
    pub(crate) const VIEW_PLAYLIST: &str = "cathar.view_playlist";
    pub(crate) const VIEW_VIZ: &str = "cathar.view_viz";
    pub(crate) const OPEN_EQ: &str = "cathar.open_eq";
    // Playback — loop / selection / A–B / listen routing (not on the transport strip).
    pub(crate) const LOOP_FILE: &str = "cathar.loop_file";
    pub(crate) const PLAY_SELECTION: &str = "cathar.play_selection";
    pub(crate) const AB_FROM_SEL: &str = "cathar.ab_from_sel";
    pub(crate) const AB_CLEAR: &str = "cathar.ab_clear";
    pub(crate) const LISTEN_STEREO: &str = "cathar.listen_stereo";
    pub(crate) const LISTEN_LEFT: &str = "cathar.listen_left";
    pub(crate) const LISTEN_RIGHT: &str = "cathar.listen_right";
    pub(crate) const LISTEN_MID: &str = "cathar.listen_mid";
}

/// Owns the native menu graph for the process lifetime.
pub(crate) struct NativeMenu {
    /// Root menu must stay alive for the OS to keep showing it.
    _menu: Menu,
    save: MenuItem,
    undo: MenuItem,
    redo: MenuItem,
    clear_selection: MenuItem,
    play_selection: MenuItem,
    heal_selection: MenuItem,
    attenuate_selection: MenuItem,
    compare_original: MenuItem,
    clear_ab: MenuItem,
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
        let clear_selection = MenuItem::with_id(
            id::CLEAR_SELECTION,
            "Clear Selection",
            false,
            Some(Accelerator::new(None, Code::Escape)),
        );
        let play_selection = MenuItem::with_id(
            id::PLAY_SELECTION,
            "Play Selection",
            false,
            Some(Accelerator::new(None, Code::KeyP)),
        );
        let heal_selection = MenuItem::with_id(
            id::HEAL_SELECTION,
            "Heal Selection",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyH)),
        );
        let attenuate_selection = MenuItem::with_id(
            id::ATTENUATE_SELECTION,
            "Attenuate Selection (−12 dB)",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyD)),
        );
        let compare_original = MenuItem::with_id(
            id::COMPARE_ORIGINAL,
            "Compare Original",
            false,
            Some(Accelerator::new(None, Code::KeyC)),
        );
        // Same id as Playback → Clear A–B (one handler).
        let clear_ab = MenuItem::with_id(
            id::AB_CLEAR,
            "Clear A–B Loop",
            false,
            Some(Accelerator::new(Some(Modifiers::SHIFT), Code::KeyL)),
        );
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &undo,
                &redo,
                &PredefinedMenuItem::separator(),
                &clear_selection,
                &play_selection,
                &heal_selection,
                &attenuate_selection,
                &PredefinedMenuItem::separator(),
                &compare_original,
                &clear_ab,
            ],
        )?;
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

        // Playback: loop + A–B set + listen (selection ops live under Edit).
        let loop_file = MenuItem::with_id(
            id::LOOP_FILE,
            "Loop File",
            true,
            Some(Accelerator::new(None, Code::KeyL)),
        );
        let ab_from = MenuItem::with_id(
            id::AB_FROM_SEL,
            "A–B Loop from Selection",
            true,
            Some(Accelerator::new(Some(Modifiers::SHIFT), Code::KeyA)),
        );
        let listen_stereo = MenuItem::with_id(id::LISTEN_STEREO, "Listen: Stereo", true, None);
        let listen_left = MenuItem::with_id(id::LISTEN_LEFT, "Listen: Left", true, None);
        let listen_right = MenuItem::with_id(id::LISTEN_RIGHT, "Listen: Right", true, None);
        let listen_mid = MenuItem::with_id(id::LISTEN_MID, "Listen: Mid (Mono)", true, None);
        let playback = Submenu::with_items(
            "Playback",
            true,
            &[
                &loop_file,
                &ab_from,
                &PredefinedMenuItem::separator(),
                &listen_stereo,
                &listen_left,
                &listen_right,
                &listen_mid,
            ],
        )?;
        menu.append(&playback)?;

        Ok(Self {
            _menu: menu,
            save,
            undo,
            redo,
            clear_selection,
            play_selection,
            heal_selection,
            attenuate_selection,
            compare_original,
            clear_ab,
            installed: false,
        })
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

    /// Enable menu items from current app state.
    pub(crate) fn set_enabled(
        &self,
        can_save: bool,
        can_undo: bool,
        can_redo: bool,
        has_selection: bool,
        can_compare: bool,
        has_ab_loop: bool,
    ) {
        self.save.set_enabled(can_save);
        self.undo.set_enabled(can_undo);
        self.redo.set_enabled(can_redo);
        self.clear_selection.set_enabled(has_selection);
        self.play_selection.set_enabled(has_selection);
        self.heal_selection.set_enabled(has_selection);
        self.attenuate_selection.set_enabled(has_selection);
        self.compare_original.set_enabled(can_compare);
        self.clear_ab.set_enabled(has_ab_loop);
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
