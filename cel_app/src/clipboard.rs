use std::sync::{LazyLock, Mutex};
use cli_clipboard::{ClipboardContext, ClipboardProvider};

static CLIPBOARD: LazyLock<Option<Mutex<ClipboardContext>>> = LazyLock::new(|| {
    if let Ok(ctx) = ClipboardContext::new() {
        Some(Mutex::new(ctx))
    } else {
        log::error!("Failed to initialize clipboard context");
        None
    }
});

pub fn set_clipboard_contents(content: &str) {
    if CLIPBOARD.is_none() { return }
    let mut clipboard = CLIPBOARD.as_ref().unwrap().lock().unwrap();
    clipboard.set_contents(content.to_string()).unwrap();
}

pub fn get_clipboard_contents() -> String {
    if CLIPBOARD.is_none() { return String::new() }
    let mut clipboard = CLIPBOARD.as_ref().unwrap().lock().unwrap();
    clipboard.get_contents().unwrap_or_else(|_| "".to_string())
}

