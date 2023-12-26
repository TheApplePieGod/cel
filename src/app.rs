use log::*;
use rust_fontconfig::FcFontCache;
use std::time;

use crate::app_state::AppState;
use crate::commands::{Commands, self};
use crate::font::Font;
use crate::renderer::Renderer;
use crate::window::Window;

pub struct App {
    window: Window,
    renderer: Renderer,
    commands: Commands,
    primary_font: Font
}

impl App {
    pub fn new() -> Self {
        let window = Window::new();
        let font_cache = FcFontCache::build();

        /*
        for font in font_cache.list() {
            warn!("Found {}", &font.0.name.as_ref().unwrap_or(&String::new()));
        }
        */

        let primary_font_name = "Martian Mono Regular";
        #[cfg(target_os = "macos")]
        let mut fallback_fonts = vec!["Courier New", "Apple Color Emoji", "Apple Symbols", "Arial Unicode MS"];
        #[cfg(target_os = "linux")]
        let mut fallback_fonts = vec!["Courier New", "Arial Unicode MS"];
        #[cfg(target_os = "windows")]
        let mut fallback_fonts = vec!["Courier New", "Segoe UI Emoji", "Arial Unicode MS"];
        fallback_fonts.insert(0, primary_font_name);

        let primary_font = match Font::new(&font_cache, &fallback_fonts) {
            Ok(font) => font,
            Err(_) => {
                panic!("Default and fallback fonts unavailable")
            }
        };

        info!("Loaded primary font '{}'", primary_font.get_primary_name());
        info!("Initialized");

        Self {
            window,
            commands: Commands::new(),
            renderer: Renderer::new(),
            primary_font
        }
    }

    pub fn run(&mut self) {
        let mut line_offset: f32 = 0.0; // todo: move
        let mut delta_time = time::Duration::new(0, 0);
        while AppState::current().borrow().running && !self.window.get_handle().should_close() {
            let frame_start = time::Instant::now();

            // Begin frame
            self.commands.poll_io();
            self.window.poll_events();
            self.window.begin_frame();

            // Handle input
            if self.window.get_key_pressed(glfw::Key::Up) {
                line_offset -= 10.0 * delta_time.as_secs_f32();
            }
            if self.window.get_key_pressed(glfw::Key::Down) {
                line_offset += 10.0 * delta_time.as_secs_f32();
            }

            // Render
            self.renderer.update_viewport_size(
                self.window.get_pixel_width(),
                self.window.get_pixel_height()
            );

            self.renderer.render(
                &mut self.primary_font,
                self.commands.get_output(),
                //&vec![String::from(text)],
                64,
                line_offset,
                true
            );

            // End frame
            self.window.end_frame();

            delta_time = time::Instant::now() - frame_start;
        }
    }
}
