use cel_core::commands::Commands;

use crate::input::Input;
use crate::terminal_widget::TerminalWidget;

pub struct TerminalContext {
    commands: Commands,
    widgets: Vec<TerminalWidget>,

    input_buffer: Vec<u8>,
    output_buffer: Vec<u8>,
    max_sequences_to_process: u32,

    debug_discrete_processing: bool
}

impl TerminalContext {
    pub fn new() -> Self {
        Self {
            commands: Commands::new(),
            widgets: vec![TerminalWidget::new()],

            input_buffer: vec![],
            output_buffer: vec![],
            max_sequences_to_process: 0,

            debug_discrete_processing: false
        }
    }

    pub fn update(&mut self, input: &Input) {
        self.handle_user_io(input);
        self.handle_process_io();
    }

    pub fn get_widgets(&mut self) -> &mut Vec<TerminalWidget> { &mut self.widgets }

    fn handle_user_io(&mut self, input: &Input) {
        self.input_buffer.extend_from_slice(input.get_input_buffer());

        if input.get_key_just_pressed(glfw::Key::F1) {
            self.debug_discrete_processing = !self.debug_discrete_processing;
        }

        self.max_sequences_to_process = std::u32::MAX;
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
        let did_split = self.commands.poll_io();

        let output = self.commands.get_output();
        self.output_buffer.extend_from_slice(&output[0]);
        for _ in 0..self.max_sequences_to_process {
            match self.widgets.last_mut().unwrap().push_chars(
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

        if did_split {
            let widget_len = self.widgets.len();
            if widget_len > 1 {
                self.widgets[widget_len - 2].set_expanded(false);
            }

            self.widgets.last_mut().unwrap().set_primary(false);

            self.widgets.push(TerminalWidget::new());
            self.widgets.last_mut().unwrap().push_chars(&output[1], false);
        }

        let last_widget = self.widgets.last_mut().unwrap();
        let last_widget_size = last_widget.get_terminal_size();

        // Next commands should obey the size of the current active widget
        self.commands.resize(last_widget_size[1], last_widget_size[0]);

        // Only need to send input to the active widget
        self.commands.send_input(&last_widget.consume_output_stream());

        self.commands.send_input(&self.input_buffer);
        self.commands.clear_output();

        self.input_buffer.clear();
    }
}
