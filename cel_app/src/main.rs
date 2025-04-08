#![windows_subsystem = "windows"]

#[cfg(target_os = "macos")]
#[macro_use]
extern crate objc;

mod app;
mod app_state;
mod logging;
mod window;
mod layout;
mod tab_group;
mod input;
mod terminal_context;
mod terminal_widget;
mod button;

use crate::{app::App, logging::ConsoleLogger};

static LOGGER: ConsoleLogger = ConsoleLogger;

fn main() {
    // Initialize logging
    match log::set_logger(&LOGGER) {
        Ok(_) => log::set_max_level(log::LevelFilter::Trace),
        Err(e) => println!("Failed to initialize logger: {}", e)
    }

    // Run the app
    let mut app = App::new();
    app.run();
}
