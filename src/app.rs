use log::*;
use rust_fontconfig::FcFontCache;

use crate::app_state::AppState;
use crate::font::Font;
use crate::renderer::Renderer;
use crate::window::Window;

pub struct App {
    window: Window,
    renderer: Renderer,
    primary_font: Font
}

impl App {
    pub fn new() -> Self {
        let window = Window::new();
        let font_cache = FcFontCache::build();
        let primary_font = match Font::new(&font_cache, "Martian Mono Regular") {
            Ok(font) => font,
            Err(_) => {
                error!("Failed to find default font, falling back to Courier New");
                match Font::new(&font_cache, "Courier New") {
                    Ok(font) => font,
                    Err(_) => panic!("Default and fallback fonts unavailable")
                }
            }
        };

        info!("Loaded font '{}'", primary_font.get_name());
        info!("Initialized");

        Self {
            window,
            renderer: Renderer::new(),
            primary_font
        }
    }

    pub fn run(&mut self) {
        while AppState::current().borrow().running && !self.window.get_handle().should_close() {
            // Begin frame
            self.window.poll_events();
            self.window.begin_frame();

            // Render
            self.renderer.update_viewport_size(
                self.window.get_pixel_width(),
                self.window.get_pixel_height()
            );
            self.renderer.render(&mut self.primary_font, "Hmello World!\nBased;)", 64);

            // End frame
            self.window.end_frame();
        }
    }
}
