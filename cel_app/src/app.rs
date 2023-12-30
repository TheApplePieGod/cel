use log::*;
use std::time;

use cel_core::{ansi::AnsiHandler, commands::Commands};
use cel_renderer::{font::{Font, FontCache}, renderer::Renderer};

use crate::app_state::AppState;
use crate::window::Window;

pub struct App {
    window: Window,
    renderer: Renderer,
    commands: Commands,
    ansi_handler: AnsiHandler,
    primary_font: Font
}

impl App {
    pub fn new() -> Self {
        let window = Window::new();
        let font_cache = FontCache::build();

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
            primary_font,
            ansi_handler: AnsiHandler::new()
        }
    }

    pub fn run(&mut self) {
        self.renderer.update_viewport_size(
            self.window.get_pixel_width(),
            self.window.get_pixel_height()
        );

        let chars_per_row = 128;
        let lines_per_screen = self.renderer.compute_max_screen_lines(
            &self.primary_font,
            chars_per_row
        );

        log::info!("CPR: {}, LPS: {}", chars_per_row, lines_per_screen);

        self.commands.resize(lines_per_screen, chars_per_row);
        self.ansi_handler.resize(chars_per_row, lines_per_screen);

        let scroll_lines_per_second = 30.0;
        let continuous_processing = false;
        let debug_line_numbers = false;
        let debug_show_cursor = true;

        let mut tail = true;
        let mut can_scroll_down = false;
        let mut line_offset: f32 = 0.0;
        let mut delta_time = time::Duration::new(0, 0);
        let mut output_buffer = vec![];
        let mut process_next_input = true;
        while AppState::current().borrow().running && !self.window.get_handle().should_close() {
            let frame_start = time::Instant::now();

            // Begin frame
            self.window.poll_events();
            self.window.begin_frame();
            self.commands.poll_io();

            let mut max_sequences: u32 = match continuous_processing {
                true => std::u32::MAX,
                false => 0
            };
            if self.window.get_key_pressed(glfw::Key::F10)
               || self.window.get_key_pressed(glfw::Key::F11)
               || self.window.get_key_pressed(glfw::Key::F12)
               || self.window.get_key_pressed(glfw::Key::F5) {
                if process_next_input {
                    if self.window.get_key_pressed(glfw::Key::F10) {
                        max_sequences = 1;
                    } else if self.window.get_key_pressed(glfw::Key::F11) {
                        max_sequences = 10;
                    } else if self.window.get_key_pressed(glfw::Key::F12) {
                        max_sequences = 100;
                    } else {
                        max_sequences = std::u32::MAX;
                    }
                    process_next_input = false;
                }
            } else {
                process_next_input = true;
            }

            output_buffer.extend_from_slice(self.commands.get_output());
            for _ in 0..max_sequences {
                match self.ansi_handler.handle_sequence(&output_buffer, !continuous_processing) {
                    Some((i, j)) => {
                        output_buffer.drain(0..(i as usize));
                        output_buffer[0].drain(0..=(j as usize));
                    },
                    None => {
                        output_buffer.clear();
                        break;
                    }
                }
            }

            self.commands.send_input(&self.ansi_handler.consume_output_stream());
            self.commands.send_input(self.window.get_input_buffer());

            // Handle input
            if self.window.get_key_pressed(glfw::Key::Up) {
                tail = false;
                line_offset -= scroll_lines_per_second * delta_time.as_secs_f32();
                if line_offset < 0.0 {
                    line_offset = 0.0;
                }
            }
            else if can_scroll_down {
                if self.window.get_key_pressed(glfw::Key::Down) {
                    line_offset += scroll_lines_per_second * delta_time.as_secs_f32();
                }
            } else {
                tail = true;
            }

            if tail {
                let buffer = &self.ansi_handler.get_terminal_state().screen_buffer;
                line_offset = ((buffer.len() + 1).max(lines_per_screen as usize) - lines_per_screen as usize) as f32;
            }

            // Render
            can_scroll_down = self.renderer.render(
                &mut self.primary_font,
                self.ansi_handler.get_terminal_state(),
                chars_per_row,
                lines_per_screen,
                line_offset,
                true,
                debug_line_numbers,
                debug_show_cursor
            );

            // End frame
            self.window.end_frame();
            self.commands.clear_output();

            delta_time = time::Instant::now() - frame_start;
        }
    }
}
