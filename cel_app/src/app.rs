use std::time;

use crate::app_state::AppState;
use crate::window::Window;

pub struct App {
    pub window: Window
}

impl App {
    pub fn new() -> Self {
        let window = Window::new();

        Self {
            window
        }
    }

    pub fn run(&mut self) {
        let scroll_lines_per_second = 10.0;

        let mut tail = true;
        let mut can_scroll_down = false;
        let mut line_offset: f32 = 0.0;
        let mut delta_time = time::Duration::new(0, 0);
        while AppState::current().borrow().running && !self.window.should_close() {
            let frame_start = time::Instant::now();

            self.window.update();
            self.window.render();

            // Handle input
            // TODO: kinda scuffed
            /*
            let term_state = &self.ansi_handler.get_terminal_state();
            let line_occupancy = match (line_offset as usize) < term_state.screen_buffer.len() {
                false => 0,
                true => term_state.screen_buffer[line_offset as usize].len()
            } / self.chars_per_line as usize + 1;
            let wrap_offset_increment = 1.0 / line_occupancy as f32;
            let line_offset_increment = scroll_lines_per_second * wrap_offset_increment * delta_time.as_secs_f32();
            if self.window.get_key_pressed(glfw::Key::Up) {
                tail = false;
                line_offset -= line_offset_increment;
                if line_offset < 0.0 {
                    line_offset = 0.0;
                }
            }
            else if can_scroll_down {
                if self.window.get_key_pressed(glfw::Key::Down) {
                    line_offset += line_offset_increment;
                }
            } else {
                tail = true;
            }

            if tail {
                let y_offset = term_state.global_cursor_home[1];
                line_offset = y_offset as f32;
                if y_offset < term_state.screen_buffer.len() {
                    // Add wrap offset
                    line_offset += term_state.global_cursor_home[0] as f32 / term_state.screen_buffer[y_offset].len() as f32;
                }
            }
            */

            delta_time = time::Instant::now() - frame_start;
        }
    }
}
