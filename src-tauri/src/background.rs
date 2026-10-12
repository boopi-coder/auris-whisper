//! Жизнь в фоне: значок в трее, горячая клавиша голосового ввода, автозапуск.
//! Пока голосовой ввод включён, закрытие окна только прячет его — приложение
//! остаётся в трее и слушает горячую клавишу.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::GlobalShortcutExt;

use crate::i18n::tr;
use crate::{dictation, AppState};

const TRAY: &str = "tray";
/// Аргумент, с которым приложение запускается при входе в систему: сразу в трей.
pub const BACKGROUND_ARG: &str = "--background";

/// Горячая клавиша по умолчанию. ⌥Space на Mac свободна; на Windows Alt+Space
/// открывает системное меню окна, поэтому там Ctrl+Alt+Space.
pub fn default_hotkey() -> String {
    if cfg!(target_os = "macos") { "Alt+Space" } else { "Control+Alt+Space" }.to_string()
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Значок в трее с меню.
pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = tray_menu(app)?;
    let mut builder = TrayIconBuilder::with_id(TRAY)
        .tooltip("Auris Whisper")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "dictate" => dictation::toggle(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let hotkey = app.state::<AppState>().settings.lock().unwrap().hotkey.clone();
    let dictate = tr("Голосовой ввод", "Voice input") + &format!("\t{}", pretty(&hotkey));
    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "show", tr("Открыть Auris Whisper", "Open Auris Whisper"), true, None::<&str>)?,
            &MenuItem::with_id(app, "dictate", dictate, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", tr("Выйти", "Quit"), true, None::<&str>)?,
        ],
    )
}

/// Обновить подписи меню (язык, горячая клавиша).
pub fn refresh_tray(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY), tray_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}

/// «Alt+Space» → «⌥ Space» на Mac, «Ctrl+Alt+Space» в других системах.
pub fn pretty(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|p| {
            let p = p.trim();
            let mac = cfg!(target_os = "macos");
            match p.to_ascii_lowercase().as_str() {
                "control" | "ctrl" => if mac { "⌃" } else { "Ctrl" }.to_string(),
                "alt" | "option" => if mac { "⌥" } else { "Alt" }.to_string(),
                "shift" => if mac { "⇧" } else { "Shift" }.to_string(),
                "super" | "meta" | "cmd" | "command" => if mac { "⌘" } else { "Win" }.to_string(),
                "commandorcontrol" | "cmdorctrl" => if mac { "⌘" } else { "Ctrl" }.to_string(),
                _ => p.strip_prefix("Key").or_else(|| p.strip_prefix("Digit")).unwrap_or(p).to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(if cfg!(target_os = "macos") { "" } else { "+" })
}

/// Включить голосовой ввод с этой клавишей (или выключить). При ошибке
/// (клавиша занята другой программой) старая регистрация не трогается.
pub fn apply_hotkey(app: &AppHandle, enabled: bool, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    if !enabled {
        return Ok(());
    }
    gs.register(hotkey).map_err(|e| {
        tr("Не получилось занять эту клавишу — возможно, её использует другая программа. ", "Could not use this key — another program may be using it. ")
            + &format!("({e})")
    })
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let al = app.autolaunch();
    let now = al.is_enabled().unwrap_or(false);
    if enabled && !now {
        al.enable().map_err(|e| e.to_string())?;
    } else if !enabled && now {
        al.disable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn autostart_enabled(app: &AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_hotkeys() {
        let p = pretty("Control+Alt+KeyD");
        if cfg!(target_os = "macos") {
            assert_eq!(p, "⌃⌥D");
        } else {
            assert_eq!(p, "Ctrl+Alt+D");
        }
    }
}
