//! The native context menu: [`core::menu_model`](crate::core::menu_model)
//! mapped onto `tauri::menu`, shown on every tray icon and on a bubble
//! right-click.
//!
//! Native check items toggle their own mark when clicked, so the menus are
//! rebuilt from state after every handled click, including clicks that
//! change nothing. Every function here that touches Tauri runs without the
//! state lock held.

use std::sync::{Arc, Mutex, PoisonError};

use tauri::menu::{
    CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
};
use tauri::{AppHandle, Manager, State, Wry};
#[cfg(not(windows))]
use tauri_plugin_autostart::{AutoLaunchManager, ManagerExt};

use crate::core::i18n::LocaleStrings;
use crate::core::menu_model::{self, MenuCommand, MenuEntry, MenuInput};
use crate::core::settings::Settings;
use crate::core::state::UpdateStatus;
use crate::runtime::{AppState, Runtime, BUBBLE_LABEL_PREFIX};

/// An owned copy of the state the menu reflects, taken under the lock.
#[derive(Clone, Debug)]
pub struct MenuSnapshot {
    settings: Settings,
    strings: LocaleStrings,
    update_status: UpdateStatus,
    update_can_apply: bool,
}

impl MenuSnapshot {
    pub fn of(state: &AppState) -> Self {
        Self {
            settings: state.settings.clone(),
            strings: state.strings.clone(),
            update_status: state.update_status,
            update_can_apply: state.update_channel.can_apply(),
        }
    }

    /// The menu tree for this snapshot and the autostart state.
    pub fn entries(&self, autostart_enabled: bool) -> Vec<MenuEntry> {
        menu_model::build(MenuInput {
            settings: &self.settings,
            strings: &self.strings,
            autostart_enabled,
            update_status: self.update_status,
            update_can_apply: self.update_can_apply,
        })
    }
}

/// The menu tree last applied to the trays, so an unchanged tree is not
/// rebuilt on every poll or countdown tick.
#[derive(Debug, Default)]
pub struct MenuCache {
    last: Mutex<Option<Vec<MenuEntry>>>,
}

impl MenuCache {
    /// Record `entries` and report whether the trays need a new menu: when
    /// forced or when the tree differs from the last one recorded.
    pub fn update(&self, entries: &[MenuEntry], force: bool) -> bool {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if !force && last.as_deref() == Some(entries) {
            return false;
        }
        *last = Some(entries.to_vec());
        true
    }
}

/// Whether the app starts at login. Reads the live entry, so a Task
/// Manager disable shows as unchecked; any read error counts as off.
pub fn autostart_enabled(app: &AppHandle) -> bool {
    autostart_state(app).unwrap_or_else(|err| {
        log::warn!("autostart state unreadable: {err}");
        false
    })
}

/// Flip the start-at-login entry. Errors are logged; the next menu rebuild
/// shows the real state either way.
pub fn toggle_autostart(app: &AppHandle) {
    let result = autostart_state(app).and_then(|enabled| set_autostart(app, !enabled));
    if let Err(err) = result {
        log::error!("autostart toggle failed: {err}");
    }
}

/// Windows bypasses the autostart plugin: its builder cannot pin the
/// current-user mode, so an elevated run would write an all-users entry
/// with an unquoted path (see `platform::windows::autostart_entry`).
#[cfg(windows)]
fn autostart_state(_app: &AppHandle) -> Result<bool, String> {
    crate::platform::windows::autostart_entry(crate::runtime::AUTOSTART_NAME)
        .map_err(|err| err.to_string())?
        .is_enabled()
        .map_err(|err| err.to_string())
}

#[cfg(windows)]
fn set_autostart(_app: &AppHandle, enable: bool) -> Result<(), String> {
    let entry = crate::platform::windows::autostart_entry(crate::runtime::AUTOSTART_NAME)
        .map_err(|err| err.to_string())?;
    let result = if enable {
        entry.enable()
    } else {
        entry.disable()
    };
    result.map_err(|err| err.to_string())
}

/// Without the plugin (it failed to set up) start at login reads as off.
#[cfg(not(windows))]
fn autostart_state(app: &AppHandle) -> Result<bool, String> {
    if app.try_state::<AutoLaunchManager>().is_none() {
        return Ok(false);
    }
    app.autolaunch().is_enabled().map_err(|err| err.to_string())
}

#[cfg(not(windows))]
fn set_autostart(app: &AppHandle, enable: bool) -> Result<(), String> {
    if app.try_state::<AutoLaunchManager>().is_none() {
        return Err("autostart plugin is not available".into());
    }
    let manager = app.autolaunch();
    let result = if enable {
        manager.enable()
    } else {
        manager.disable()
    };
    result.map_err(|err| err.to_string())
}

/// Build a native menu from the tree, keeping order, labels, check marks
/// and greyed states. Hint rows are disabled items with generated ids.
pub fn build(app: &AppHandle, entries: &[MenuEntry]) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        menu.append(item(app, entry)?.as_ref())?;
    }
    Ok(menu)
}

fn item(app: &AppHandle, entry: &MenuEntry) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    Ok(match entry {
        MenuEntry::Action {
            command,
            label,
            enabled,
        } => Box::new(MenuItem::with_id(
            app,
            command.id(),
            label,
            *enabled,
            None::<&str>,
        )?),
        MenuEntry::Check {
            command,
            label,
            checked,
            enabled,
        } => Box::new(CheckMenuItem::with_id(
            app,
            command.id(),
            label,
            *enabled,
            *checked,
            None::<&str>,
        )?),
        MenuEntry::Hint { label } => Box::new(MenuItem::new(app, label, false, None::<&str>)?),
        MenuEntry::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        MenuEntry::Submenu { label, entries } => {
            let submenu = Submenu::new(app, label, true)?;
            for child in entries {
                submenu.append(item(app, child)?.as_ref())?;
            }
            Box::new(submenu)
        }
    })
}

/// The one handler for every menu click, from a tray or a bubble menu.
/// Runs on the main thread.
pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref();
    let Some(command) = MenuCommand::parse(id) else {
        log::debug!("menu: ignoring id {id:?}");
        return;
    };
    let Some(runtime) = app.try_state::<Arc<Runtime>>() else {
        return;
    };
    let runtime = Arc::clone(&runtime);
    runtime.on_menu_command(command);
    if !matches!(command, MenuCommand::Restart | MenuCommand::Exit) {
        runtime.refresh_shell(true);
    }
}

/// Show the context menu on a bubble at the cursor.
#[tauri::command]
pub async fn show_context_menu(
    app: AppHandle,
    runtime: State<'_, Arc<Runtime>>,
    label: String,
) -> Result<(), String> {
    if !label.starts_with(BUBBLE_LABEL_PREFIX) {
        return Err("not a bubble window".into());
    }
    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "no such bubble window".to_string())?;
    let snapshot = runtime.with_state(|s| MenuSnapshot::of(s));
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let entries = snapshot.entries(autostart_enabled(&handle));
        let menu = match build(&handle, &entries) {
            Ok(menu) => menu,
            Err(err) => {
                log::error!("context menu could not be built: {err}");
                return;
            }
        };
        // The popup only dismisses on an outside click when its owner is
        // the foreground window.
        #[cfg(windows)]
        crate::platform::windows::set_foreground(&window);
        if let Err(err) = window.popup_menu(&menu) {
            log::error!(
                "context menu could not be shown on {}: {err}",
                window.label()
            );
        }
    })
    .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::i18n::I18n;

    fn snapshot(settings: Settings) -> MenuSnapshot {
        MenuSnapshot {
            settings,
            strings: I18n::load().strings().clone(),
            update_status: UpdateStatus::Idle,
            update_can_apply: true,
        }
    }

    #[test]
    fn cache_skips_an_unchanged_tree_unless_forced() {
        let cache = MenuCache::default();
        let entries = snapshot(Settings::default()).entries(false);
        assert!(
            cache.update(&entries, false),
            "first tree is always applied"
        );
        assert!(!cache.update(&entries, false));
        assert!(
            cache.update(&entries, true),
            "a check-item click rebuilds even when nothing changed"
        );
        let toggled = snapshot(Settings::default()).entries(true);
        assert!(cache.update(&toggled, false));
        assert!(!cache.update(&toggled, false));
    }

    #[test]
    fn snapshot_entries_follow_the_autostart_state() {
        let snap = snapshot(Settings::default());
        let autostart = |entries: &[MenuEntry]| -> Option<bool> {
            fn find(entries: &[MenuEntry]) -> Option<bool> {
                entries.iter().find_map(|e| match e {
                    MenuEntry::Check {
                        command: MenuCommand::Autostart,
                        checked,
                        ..
                    } => Some(*checked),
                    MenuEntry::Submenu { entries, .. } => find(entries),
                    _ => None,
                })
            }
            find(entries)
        };
        assert_eq!(autostart(&snap.entries(true)), Some(true));
        assert_eq!(autostart(&snap.entries(false)), Some(false));
    }
}
