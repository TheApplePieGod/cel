use cel_core::ansi::{AnsiBuilder, AnsiHandler, CellContent, ScreenBuffer, TerminalState};
use log::{Record, Level, Metadata};

static LOGGER: ConsoleLogger = ConsoleLogger;

pub struct ConsoleLogger;

impl log::Log for ConsoleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            println!(
                "[{}:{}] {}: {}",
                record.target(),
                record.line().unwrap_or(0),
                record.level(),
                record.args()
            )
        }
    }

    fn flush(&self) {}
}


fn setup() -> (AnsiHandler, AnsiBuilder) {
    match log::set_logger(&LOGGER) {
        Ok(_) => log::set_max_level(log::LevelFilter::Trace),
        Err(_) => {}
    }

    let handler = AnsiHandler::new(5, 5);
    let builder = AnsiBuilder::new();

    (handler, builder)
}

pub fn get_final_state(builder: AnsiBuilder) -> TerminalState {
    let (mut handler, _) = setup();
    let stream = builder.build_stream();
    handler.handle_sequence_bytes(&stream, false);

    handler.get_terminal_state().clone()
}

fn print_buffer_contents(buf: &Vec<Vec<String>>) -> String {
    let mut res = format!("<len={}>\n", buf.len());
    for (i, line) in buf.iter().enumerate() {
        res.push_str(&format!("[{}] ", line.len()));
        for elem in line {
            res.push_str(&format!("{} ", elem));
        }
        if i != buf.len() - 1 {
            res.push('\n');
        }
    }

    res
}

pub fn assert_buffer_chars_eq(test: &ScreenBuffer, expect: &Vec<Vec<&str>>)  {
    let test_str = print_buffer_contents(&test.iter().map(|l|
        l.iter().map(|e|
            match &e.elem {
                CellContent::Char(c, _) => match c {
                    '\0' => ".".to_string(),
                    _ => c.to_string()
                },
                CellContent::Grapheme(str, _) => str.clone(),
                CellContent::Continuation(width) => format!("+{}", width),
                CellContent::Empty => ".".to_string(),
            }
        ).collect()
    ).collect());
    let expect_str = print_buffer_contents(&expect.iter().map(|l|
        l.iter().map(|e| e.to_string()).collect()
    ).collect());
    assert!(
        test_str == expect_str,
        "Buffers do not match!\nTest: {}\n\n==========\n\nExpect:{}",
        test_str, expect_str
    );
}

