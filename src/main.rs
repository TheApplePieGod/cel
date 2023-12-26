mod ansi;
mod app;
mod app_state;
mod commands;
mod font;
mod logging;
mod renderer;
mod texture;
mod util;
mod window;

extern crate glfw;
extern crate gl;
extern crate log;

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
