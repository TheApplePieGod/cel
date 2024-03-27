use cel_core::ansi::AnsiHandler;
use cel_renderer::renderer::Renderer;

use crate::layout::LayoutPosition;

pub struct TerminalWidget {
    char_buffer: Vec<u8>,
    ansi_handler: AnsiHandler,
    line_offset: f32,
    chars_per_line: u32,
    lines_per_screen: u32,
    wrap: bool,

    debug_line_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new() -> Self {
        Self {
            char_buffer: vec![],
            ansi_handler: AnsiHandler::new(),
            line_offset: 0.0,
            chars_per_line: 180,
            lines_per_screen: 1,
            wrap: true,

            debug_line_number: false,
            debug_show_cursor: false
        }
    }

    pub fn consume_output_stream(&mut self) -> Vec<u8> {
        self.ansi_handler.consume_output_stream()
    }

    pub fn push_chars(&mut self, chars: &[u8]) {
        //self.char_buffer.push(char);
        self.ansi_handler.handle_sequence_bytes(chars, false);
    }

    pub fn render(&mut self, renderer: &mut Renderer, position: &LayoutPosition) {
        let width_px = renderer.get_pixel_width() as f32 * position.max_size[0];
        let pixel_to_char_ratio = 18;
        let max_chars = width_px as u32 / pixel_to_char_ratio;
        let max_lines = renderer.compute_max_lines(max_chars, position.max_size[1]);

        if max_chars != self.chars_per_line || max_lines != self.lines_per_screen {
            self.chars_per_line = max_chars;
            self.lines_per_screen = max_lines;

            //log::info!("CPL: {}, LPS: {}", self.chars_per_line, self.lines_per_screen);

            self.ansi_handler.resize(self.chars_per_line, self.lines_per_screen);
        }

        renderer.render(
            &position.offset,
            &self.ansi_handler.get_terminal_state(),
            max_chars,
            max_lines,
            self.line_offset,
            self.wrap,
            self.debug_line_number,
            self.debug_show_cursor
        );
    }

    pub fn get_terminal_size(&self) -> [u32; 2] {
        [self.chars_per_line, self.lines_per_screen]
    }
}
