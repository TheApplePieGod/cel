use std::time;

use crate::app_state::AppState;
use crate::window::Window;

pub struct App {
    pub window: Window
}

const MIN_DELTA_TIME_NS: u128 = 4e6 as u128;

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
            if delta_time.as_nanos() < MIN_DELTA_TIME_NS {
                let sleep_time = (MIN_DELTA_TIME_NS - delta_time.as_nanos()) as u32;
                std::thread::sleep(std::time::Duration::new(0, sleep_time));
            }
            delta_time = time::Instant::now() - frame_start;

            //log::warn!("DT: {}ms", delta_time.as_millis());
        }
    }
}
