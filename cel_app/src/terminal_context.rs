use cel_renderer::renderer::Renderer;
use cel_core::ansi::{TerminalState, AnsiHandler};
use cel_core::commands::Commands;

use crate::input::Input;
use crate::layout::LayoutPosition;
use crate::window::Window;

pub struct TerminalContext {
    commands: Commands,
    ansi_handler: AnsiHandler,
    chars_per_line: u32,
    lines_per_screen: u32,
    line_offset: f32,
    wrap: bool,
    focused: bool,

    output_buffer: Vec<u8>,
    input_buffer: Vec<u8>,
    max_sequences_to_process: u32,
    needs_resize: bool,

    debug_discrete_processing: bool,
    debug_line_number: bool,
    debug_show_cursor: bool
}

impl TerminalContext {
    pub fn new() -> Self {
        Self {
            commands: Commands::new(),
            ansi_handler: AnsiHandler::new(),
            chars_per_line: 180,
            lines_per_screen: 1,
            line_offset: 0.0,
            wrap: true,
            focused: true,

            output_buffer: vec![],
            input_buffer: vec![],
            max_sequences_to_process: 0,
            needs_resize: false,

            debug_discrete_processing: false,
            debug_line_number: false,
            debug_show_cursor: false
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn send_input(&mut self, slice: &[u8]) {
        self.input_buffer.extend_from_slice(slice);
    }

    pub fn update(&mut self, input: &Input) {
        if self.needs_resize {
            self.resize();
        }

        self.handle_user_io(input);
        self.handle_process_io();
    }

    pub fn render(&mut self, renderer: &mut Renderer, position: &LayoutPosition) {
        let max_lines = renderer.compute_max_screen_lines(self.chars_per_line);

        if max_lines != self.lines_per_screen {
            self.needs_resize = true;
            self.lines_per_screen = max_lines;
        }

        renderer.render(
            &position.offset,
            &self.ansi_handler.get_terminal_state(),
            self.chars_per_line,
            max_lines,
            self.line_offset,
            self.wrap,
            self.debug_line_number,
            self.debug_show_cursor
        );
    }

    pub fn on_window_resized(&mut self, new_size: [i32; 2]) {
        let pixel_to_char_ratio = 10;
        self.chars_per_line = new_size[0] as u32 / pixel_to_char_ratio;

        self.needs_resize = true;
    }

    fn resize(&mut self) {
        log::info!("CPL: {}, LPS: {}", self.chars_per_line, self.lines_per_screen);

        self.commands.resize(self.lines_per_screen, self.chars_per_line);
        self.ansi_handler.resize(self.chars_per_line, self.lines_per_screen);

        self.needs_resize = false;
    }

    fn handle_user_io(&mut self, input: &Input) {
        self.max_sequences_to_process = std::u32::MAX;

        if !self.focused {
            return;
        }

        self.input_buffer.extend_from_slice(input.get_input_buffer());

        if self.debug_discrete_processing {
            self.max_sequences_to_process = 0;

            if input.get_key_just_pressed(glfw::Key::F10) {
                self.max_sequences_to_process = 1;
            } else if input.get_key_just_pressed(glfw::Key::F11) {
                self.max_sequences_to_process = 10;
            } else if input.get_key_just_pressed(glfw::Key::F12) {
                self.max_sequences_to_process = 100;
            } else if input.get_key_just_pressed(glfw::Key::F5) {
                self.max_sequences_to_process = std::u32::MAX;
            }
        }
    }

    fn handle_process_io(&mut self) {
        self.commands.poll_io();

        self.output_buffer.extend_from_slice(self.commands.get_output());
        for _ in 0..self.max_sequences_to_process {
            match self.ansi_handler.handle_sequence_bytes(
                &self.output_buffer,
                self.debug_discrete_processing
            ) {
                Some(i) => {
                    self.output_buffer.drain(0..=(i as usize));
                },
                None => {
                    self.output_buffer.clear();
                    break;
                }
            }
        }

        self.commands.send_input(&self.ansi_handler.consume_output_stream());
        self.commands.send_input(&self.input_buffer);

        self.input_buffer.clear();
        self.commands.clear_output();
    }
}
