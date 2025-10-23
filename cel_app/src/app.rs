use std::time;

use crate::app_state::AppState;
use crate::window::Window;

pub struct App {
    pub window: Window
}

const DT_60FPS_NS: u128 = 16666666 as u128;

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

            self.window.update_and_render(delta_time.as_secs_f64() * 1000.0);

            // Lock render updates to refresh rate
            let refresh_rate = self.window.get_monitor_info()
                .map(|info| ((1.0 / info.refresh_rate as f32) * 1.0e9) as u128)
                .unwrap_or(DT_60FPS_NS);
            delta_time = time::Instant::now() - frame_start;
            if delta_time.as_nanos() < refresh_rate {
                let sleep_time = (refresh_rate - delta_time.as_nanos()) as u32;
                std::thread::sleep(std::time::Duration::new(0, sleep_time));
            }
            delta_time = time::Instant::now() - frame_start;

            //log::warn!("DT: {}ms", delta_time.as_millis());
        }
    }
}
