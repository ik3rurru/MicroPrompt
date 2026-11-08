//! MicroPrompt: the Teleprompter's own global shortcut on Windows
//! (`labs.teleprompter_hotkey`, unbound by default). It is a system hotkey
//! (`RegisterHotKey`) on a thread of its own rather than another binding in
//! Hex's keyboard hook, so it works with the window closed and never
//! touches the dictation shortcuts' state machine; Settings keeps it from
//! sharing a key with them (`HotkeySlot::Teleprompter`). While the window
//! records a shortcut, a press does nothing, so the chord can be recorded
//! again. Linux binds `hex teleprompter toggle` in the desktop instead.

use std::sync::Mutex;
use std::sync::mpsc;

use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey, UnregisterHotKey,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_HOTKEY, WM_QUIT, WM_USER,
};

use crate::app_settings::{AppSettings, HotkeyBinding};
use crate::windows::input::WindowsHotkey;

/// The registered shortcut and the thread whose queue receives it.
static CURRENT: Mutex<Option<(HotkeyBinding, u32)>> = Mutex::new(None);

/// Registers the saved shortcut while the Lab is on, replacing an older one;
/// called at launch and after every settings change.
pub(crate) fn apply(settings: &AppSettings) {
    let wanted = settings
        .labs
        .teleprompter
        .then(|| settings.labs.teleprompter_hotkey.clone())
        .flatten();
    let mut current = super::lock(&CURRENT);
    if current.as_ref().map(|(binding, _)| binding) == wanted.as_ref() {
        return;
    }
    if let Some((_, thread)) = current.take() {
        unsafe { PostThreadMessageW(thread, WM_QUIT, 0, 0) };
    }
    let Some(binding) = wanted else {
        return;
    };
    match register(&binding) {
        Ok(thread) => *current = Some((binding, thread)),
        Err(error) => {
            tracing::warn!(%error, "the teleprompter shortcut is not registered");
            super::update(|live| {
                live.view.error = Some(format!("The shortcut isn't available: {error}"));
            });
        }
    }
}

/// `RegisterHotKey`'s modifiers and key for `binding`: sides don't matter
/// to a system hotkey.
fn hotkey(binding: &HotkeyBinding) -> Result<(u32, u32), String> {
    let hotkey = WindowsHotkey::from(binding);
    let key = hotkey.key.as_deref().ok_or("it needs a key")?;
    let vk = crate::windows::input::virtual_key(key).map_err(|error| error.to_string())?;
    let mut modifiers = MOD_NOREPEAT;
    for (held, flag) in [
        (hotkey.control.is_some(), MOD_CONTROL),
        (hotkey.alt.is_some(), MOD_ALT),
        (hotkey.shift.is_some(), MOD_SHIFT),
        (hotkey.windows.is_some(), MOD_WIN),
    ] {
        if held {
            modifiers |= flag;
        }
    }
    Ok((modifiers, vk))
}

/// Starts the thread that owns the hotkey; its id once it is registered.
fn register(binding: &HotkeyBinding) -> Result<u32, String> {
    let (modifiers, vk) = hotkey(binding)?;
    let (ready, registered) = mpsc::channel();
    std::thread::Builder::new()
        .name("teleprompter-hotkey".into())
        .spawn(move || unsafe {
            let mut message: MSG = std::mem::zeroed();
            // Creates the queue before anyone posts to it.
            PeekMessageW(
                &mut message,
                std::ptr::null_mut(),
                WM_USER,
                WM_USER,
                PM_NOREMOVE,
            );
            if RegisterHotKey(std::ptr::null_mut(), 1, modifiers, vk) == 0 {
                let _ = ready.send(Err(
                    "another app already uses it (RegisterHotKey failed)".to_string()
                ));
                return;
            }
            let _ = ready.send(Ok(GetCurrentThreadId()));
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                if message.message == WM_HOTKEY && !crate::windows::input::capture_inhibited() {
                    super::toggle();
                }
            }
            UnregisterHotKey(std::ptr::null_mut(), 1);
        })
        .map_err(|error| error.to_string())?;
    registered
        .recv()
        .map_err(|_| "its thread stopped".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chord_becomes_modifiers_and_a_key() {
        let binding: HotkeyBinding = serde_json::from_value(serde_json::json!({ "keys": [
            { "modifier": "control", "side": "any" },
            { "modifier": "option", "side": "any" },
            { "code": 17, "label": "T" },
        ]}))
        .unwrap();
        let (modifiers, vk) = hotkey(&binding).unwrap();
        assert_eq!(modifiers, MOD_NOREPEAT | MOD_CONTROL | MOD_ALT);
        assert_eq!(vk, u32::from(b'T'));
        let alone: HotkeyBinding = serde_json::from_value(serde_json::json!({ "keys": [
            { "modifier": "control", "side": "any" },
        ]}))
        .unwrap();
        assert!(hotkey(&alone).is_err());
    }
}
