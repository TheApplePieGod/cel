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
        let mut delta_time = time::Duration::new(0, 0);
        while AppState::current().borrow().running && !self.window.should_close() {
            let frame_start = time::Instant::now();

            self.window.update_and_render();

            delta_time = time::Instant::now() - frame_start;
            //log::warn!("DT: {}ms", delta_time.as_millis());
        }
    }
}
