//! Вставка текста туда, где стоит курсор, — в любую программу.
//!
//! Печатать посимвольно ненадёжно (кириллица, раскладки, медленные поля),
//! поэтому кладём текст в буфер обмена, жмём «Вставить» (⌘V / Ctrl+V) и
//! возвращаем в буфер то, что там было.
//!
//! macOS разрешает нажимать клавиши за пользователя только программам из
//! «Конфиденциальность и безопасность → Универсальный доступ».

use std::time::Duration;

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::i18n::tr;

/// Как прошла вставка.
pub enum Inserted {
    /// Текст вставлен в программу с курсором.
    Pasted,
    /// Нажать «Вставить» нельзя — текст оставлен в буфере обмена.
    Clipboard(String),
}

/// Содержимое буфера обмена, которое вернём на место после вставки.
enum Saved {
    Text(String),
    Image(arboard::ImageData<'static>),
    Nothing,
}

pub fn insert(text: &str) -> Result<Inserted, String> {
    let mut clip = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let saved = match clip.get_text() {
        Ok(t) => Saved::Text(t),
        Err(_) => match clip.get_image() {
            Ok(i) => Saved::Image(i.to_owned_img()),
            Err(_) => Saved::Nothing,
        },
    };
    clip.set_text(text.to_string()).map_err(|e| e.to_string())?;

    if !can_send_keys() {
        return Ok(Inserted::Clipboard(if cfg!(target_os = "macos") {
            tr(
                "Текст в буфере обмена — вставьте его (⌘V). Чтобы он вставлялся сам, разрешите Auris Whisper в «Системные настройки → Конфиденциальность и безопасность → Универсальный доступ».",
                "The text is on the clipboard — paste it (⌘V). To have it pasted automatically, allow Auris Whisper in System Settings → Privacy & Security → Accessibility.",
            )
        } else {
            tr(
                "Текст в буфере обмена — вставьте его (Ctrl+V). В сеансе Wayland программы не могут нажимать клавиши за пользователя.",
                "The text is on the clipboard — paste it (Ctrl+V). Under Wayland, programs cannot press keys for the user.",
            )
        }));
    }
    let pasted = press_paste();
    // Программе нужно время, чтобы забрать текст из буфера; на Linux буфер
    // к тому же живёт, пока жив наш объект Clipboard.
    std::thread::sleep(Duration::from_millis(350));
    if let Err(e) = pasted {
        return Ok(Inserted::Clipboard(e));
    }
    let _ = match saved {
        Saved::Text(t) => clip.set_text(t),
        Saved::Image(i) => clip.set_image(i),
        Saved::Nothing => clip.clear(),
    };
    Ok(Inserted::Pasted)
}

fn press_paste() -> Result<(), String> {
    let fail = |e: String| {
        tr("Не удалось вставить текст — он в буфере обмена. ", "Could not paste the text — it is on the clipboard. ") + &e
    };
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| fail(e.to_string()))?;
    let modifier = if cfg!(target_os = "macos") { Key::Meta } else { Key::Control };
    // Модификаторы горячей клавиши могут быть ещё зажаты — отпускаем их,
    // иначе вместо «Вставить» получится другая комбинация.
    for k in [Key::Shift, Key::Alt, Key::Control, Key::Meta] {
        let _ = enigo.key(k, Direction::Release);
    }
    enigo.key(modifier, Direction::Press).map_err(|e| fail(e.to_string()))?;
    let r = enigo.key(paste_key(), Direction::Click);
    let _ = enigo.key(modifier, Direction::Release);
    r.map_err(|e| fail(e.to_string()))
}

/// Клавиша V физически — не буква «v» текущей раскладки: в русской её нет,
/// и ⌘V / Ctrl+V тогда не срабатывает.
fn paste_key() -> Key {
    #[cfg(target_os = "macos")]
    {
        Key::Other(9) // kVK_ANSI_V
    }
    #[cfg(target_os = "windows")]
    {
        Key::V
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Key::Unicode('v')
    }
}

/// Можно ли нажимать клавиши за пользователя.
pub fn can_send_keys() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::trusted(false)
    }
    #[cfg(target_os = "linux")]
    {
        // Wayland не даёт программам нажимать клавиши в чужих окнах.
        std::env::var("XDG_SESSION_TYPE").map_or(true, |t| t != "wayland")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        true
    }
}

/// macOS: показать системный запрос на «Универсальный доступ».
pub fn request_access() -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::trusted(true)
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

/// Открыть настройки, где разрешают управлять компьютером.
pub fn open_access_settings() -> Result<(), String> {
    if cfg!(target_os = "macos") {
        tauri_plugin_opener::open_url(
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            None::<&str>,
        )
        .map_err(|e| e.to_string())
    } else {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use core_foundation_sys::base::{CFRelease, CFTypeRef};
    use core_foundation_sys::dictionary::{
        kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks, CFDictionaryCreate, CFDictionaryRef,
    };
    use core_foundation_sys::number::kCFBooleanTrue;
    use core_foundation_sys::string::CFStringRef;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        static kAXTrustedCheckOptionPrompt: CFStringRef;
    }

    /// Есть ли разрешение; `prompt` — заодно показать системный запрос.
    pub fn trusted(prompt: bool) -> bool {
        unsafe {
            if !prompt {
                return AXIsProcessTrusted();
            }
            let keys = [kAXTrustedCheckOptionPrompt as CFTypeRef];
            let values = [kCFBooleanTrue as CFTypeRef];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            let ok = AXIsProcessTrustedWithOptions(dict);
            CFRelease(dict as CFTypeRef);
            ok
        }
    }
}
