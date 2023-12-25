use log::{Record, Level, Metadata};

pub struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!(
                "[{} - {}:{}] {}: {}",
                chrono::offset::Local::now().format("%I:%M:%S %p"),
                record.target(),
                record.line().unwrap_or(0),
                record.level(),
                record.args()
            )
        }
    }

    fn flush(&self) {}
}
