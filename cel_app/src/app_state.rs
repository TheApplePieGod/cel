use std::{cell::RefCell, rc::Rc};
use cel_renderer::font::Font;

thread_local!(static APP_STATE: Rc<RefCell<AppState>> = Rc::new(RefCell::new(AppState::new())));

pub struct AppState {
    pub running: bool,
    pub font: Rc<RefCell<Font>>
}

impl AppState {
    fn new() -> Self {
        let primary_font_name = "Martian Mono Regular";
        let secondary_font_name = "Hack Nerd Font Mono Regular";
        #[cfg(target_os = "macos")]
        let mut fallback_fonts = vec!["Courier New", "Apple Color Emoji", "Apple Symbols", "Arial Unicode MS"];
        #[cfg(target_os = "linux")]
        let mut fallback_fonts = vec!["Courier New", "Arial Unicode MS"];
        #[cfg(target_os = "windows")]
        let mut fallback_fonts = vec!["Courier New", "Segoe UI Emoji", "Arial Unicode MS"];

        // TODO: this is confusing and messy, and we need dynamic font search / remap
        fallback_fonts.insert(0, secondary_font_name);
        fallback_fonts.insert(0, primary_font_name);

        let primary_font = match Font::new(&fallback_fonts) {
            Ok(font) => font,
            Err(_) => {
                panic!("Default and fallback fonts unavailable")
            }
        };

        log::info!("Loaded primary font '{}'", primary_font.get_primary_name());

        Self {
            running: true,
            font: Rc::new(RefCell::new(primary_font))
        }
    }

    pub fn current() -> Rc<RefCell<AppState>> {
        APP_STATE.with(|s| s.clone())
    }
}
