use std::{fs::OpenOptions, path::PathBuf, io::Write};

use log::{Record, Level, Metadata};
use cel_core::config::get_config_dir;

pub struct ConsoleLogger {
    log_path: PathBuf
}

impl ConsoleLogger {
    pub fn new() -> Self {
        let mut log_path = get_config_dir();
        log_path.push("log.txt");

        Self {
            log_path
        }
    }

    pub fn get_log_path(&self) -> &str { self.log_path.to_str().unwrap() }
}

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        if cfg!(debug_assertions) {
            metadata.level() <= Level::Trace
        } else {
            metadata.level() <= Level::Info
        }
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return
        }

        let formatted = format!(
            "[{} - {}:{}] {}: {}",
            chrono::offset::Local::now().format("%I:%M:%S %p"),
            record.target(),
            record.line().unwrap_or(0),
            record.level(),
            record.args()
        );
        println!("{}", formatted);

        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.log_path);

        if let Ok(mut file) = file {
            let _ = writeln!(file, "{}", formatted);
        }
    }

    fn flush(&self) {}
}
