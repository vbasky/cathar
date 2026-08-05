//! OS-native application menus via [`muda`] (macOS menu bar, Windows window menu).
//!
//! Keeps File / Edit / View out of the egui client area so we don't double up
//! with the system chrome.

use muda::accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use raw_window_handle::HasWindowHandle;

use crate::prefs::MAX_RECENT;

/// Menu action identifiers (stable string ids for [`MenuEvent`]).
pub(crate) mod id {
    pub(crate) const OPEN: &str = "cathar.open";
    pub(crate) const OPEN_PLAYLIST: &str = "cathar.open_playlist";
    pub(crate) const IMPORT_M3U: &str = "cathar.import_m3u";
    pub(crate) const EXPORT_M3U: &str = "cathar.export_m3u";
    pub(crate) const REVEAL: &str = "cathar.reveal";
    pub(crate) const CLEAR_RECENT: &str = "cathar.clear_recent";
    pub(crate) const OPEN_LAST_ON_LAUNCH: &str = "cathar.open_last_on_launch";
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
    // Playback — loop / selection / A–B / listen routing / queue behaviour.
    pub(crate) const LOOP_FILE: &str = "cathar.loop_file";
    pub(crate) const SHUFFLE: &str = "cathar.shuffle";
    pub(crate) const PLAYLIST_AUTO: &str = "cathar.playlist_auto";
    pub(crate) const PLAYLIST_WRAP: &str = "cathar.playlist_wrap";
    pub(crate) const PLAY_SELECTION: &str = "cathar.play_selection";
    pub(crate) const AB_FROM_SEL: &str = "cathar.ab_from_sel";
    pub(crate) const AB_CLEAR: &str = "cathar.ab_clear";
    pub(crate) const LISTEN_STEREO: &str = "cathar.listen_stereo";
    pub(crate) const LISTEN_LEFT: &str = "cathar.listen_left";
    pub(crate) const LISTEN_RIGHT: &str = "cathar.listen_right";
    pub(crate) const LISTEN_MID: &str = "cathar.listen_mid";
    pub(crate) const PREV_TRACK: &str = "cathar.prev_track";
    pub(crate) const NEXT_TRACK: &str = "cathar.next_track";

    /// Stable id for Open Recent slot `i` (0-based).
    pub(crate) fn recent(i: usize) -> String {
        format!("cathar.recent.{i}")
    }
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
    export_m3u: MenuItem,
    reveal: MenuItem,
    recent_items: Vec<MenuItem>,
    clear_recent: MenuItem,
    open_last_on_launch: CheckMenuItem,
    loop_file: CheckMenuItem,
    shuffle: CheckMenuItem,
    playlist_auto: CheckMenuItem,
    playlist_wrap: CheckMenuItem,
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
        let export_m3u = MenuItem::with_id(id::EXPORT_M3U, "Export Playlist as M3U…", false, None);
        let reveal = MenuItem::with_id(
            id::REVEAL,
            #[cfg(target_os = "macos")]
            "Reveal in Finder",
            #[cfg(target_os = "windows")]
            "Show in Explorer",
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            "Show in File Manager",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL | Modifiers::SHIFT), Code::KeyR)),
        );
        let save = MenuItem::with_id(
            id::SAVE,
            "Save…",
            false,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::KeyS)),
        );

        // Open Recent — fixed slots updated each frame via set_text / set_enabled.
        let mut recent_items = Vec::with_capacity(MAX_RECENT);
        for i in 0..MAX_RECENT {
            recent_items.push(MenuItem::with_id(
                id::recent(i),
                if i == 0 { "No Recent Files" } else { " " },
                false,
                None,
            ));
        }
        let clear_recent = MenuItem::with_id(id::CLEAR_RECENT, "Clear Menu", false, None);
        let open_recent = Submenu::new("Open Recent", true);
        for item in &recent_items {
            open_recent.append(item)?;
        }
        open_recent.append(&PredefinedMenuItem::separator())?;
        open_recent.append(&clear_recent)?;

        let open_last_on_launch = CheckMenuItem::with_id(
            id::OPEN_LAST_ON_LAUNCH,
            "Open Last Track on Launch",
            true,
            true, // default on; app syncs from prefs each frame
            None,
        );

        #[cfg(target_os = "macos")]
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &open,
                &open_recent,
                &open_last_on_launch,
                &PredefinedMenuItem::separator(),
                &open_playlist,
                &import_m3u,
                &export_m3u,
                &PredefinedMenuItem::separator(),
                &reveal,
                &save,
            ],
        )?;
        #[cfg(not(target_os = "macos"))]
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &open,
                &open_recent,
                &open_last_on_launch,
                &PredefinedMenuItem::separator(),
                &open_playlist,
                &import_m3u,
                &export_m3u,
                &PredefinedMenuItem::separator(),
                &reveal,
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

        // Playback: transport queue + loop / shuffle / A–B / listen.
        let prev_track = MenuItem::with_id(
            id::PREV_TRACK,
            "Previous Track",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::ArrowLeft)),
        );
        let next_track = MenuItem::with_id(
            id::NEXT_TRACK,
            "Next Track",
            true,
            Some(Accelerator::new(Some(CMD_OR_CTRL), Code::ArrowRight)),
        );
        let loop_file = CheckMenuItem::with_id(
            id::LOOP_FILE,
            "Loop Track",
            true,
            false,
            Some(Accelerator::new(None, Code::KeyL)),
        );
        let shuffle = CheckMenuItem::with_id(
            id::SHUFFLE,
            "Shuffle",
            true,
            false,
            Some(Accelerator::new(Some(Modifiers::SHIFT), Code::KeyS)),
        );
        let playlist_auto =
            CheckMenuItem::with_id(id::PLAYLIST_AUTO, "Auto-Advance Playlist", true, true, None);
        let playlist_wrap =
            CheckMenuItem::with_id(id::PLAYLIST_WRAP, "Repeat Playlist", true, true, None);
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
                &prev_track,
                &next_track,
                &PredefinedMenuItem::separator(),
                &loop_file,
                &shuffle,
                &playlist_auto,
                &playlist_wrap,
                &PredefinedMenuItem::separator(),
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
            export_m3u,
            reveal,
            recent_items,
            clear_recent,
            open_last_on_launch,
            loop_file,
            shuffle,
            playlist_auto,
            playlist_wrap,
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_enabled(
        &self,
        can_save: bool,
        can_undo: bool,
        can_redo: bool,
        has_selection: bool,
        can_compare: bool,
        has_ab_loop: bool,
        has_file: bool,
        has_playlist: bool,
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
        self.reveal.set_enabled(has_file);
        self.export_m3u.set_enabled(has_playlist);
    }

    /// Sync checkmarks for loop / shuffle / queue behaviour.
    pub(crate) fn set_playback_checks(
        &self,
        loop_file: bool,
        shuffle: bool,
        auto_advance: bool,
        wrap: bool,
        open_last_on_launch: bool,
    ) {
        self.loop_file.set_checked(loop_file);
        self.shuffle.set_checked(shuffle);
        self.playlist_auto.set_checked(auto_advance);
        self.playlist_wrap.set_checked(wrap);
        self.open_last_on_launch.set_checked(open_last_on_launch);
    }

    /// Refresh Open Recent labels from prefs (`(label, exists)` most-recent first).
    pub(crate) fn set_recent(&self, entries: &[(String, bool)]) {
        if entries.is_empty() {
            for (i, item) in self.recent_items.iter().enumerate() {
                if i == 0 {
                    item.set_text("No Recent Files");
                    item.set_enabled(false);
                } else {
                    item.set_text(" ");
                    item.set_enabled(false);
                }
            }
            self.clear_recent.set_enabled(false);
            return;
        }
        for (i, item) in self.recent_items.iter().enumerate() {
            if let Some((name, exists)) = entries.get(i) {
                // Numbered like Finder / Safari for muscle memory.
                item.set_text(format!("  {n}  {name}", n = i + 1, name = name));
                item.set_enabled(*exists);
            } else {
                item.set_text(" ");
                item.set_enabled(false);
            }
        }
        self.clear_recent.set_enabled(true);
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

/// Parse `cathar.recent.N` → slot index.
pub(crate) fn parse_recent_id(id: &str) -> Option<usize> {
    id.strip_prefix("cathar.recent.")?.parse().ok()
}
