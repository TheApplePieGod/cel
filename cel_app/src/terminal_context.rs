use cel_core::commands::Commands;

use crate::input::Input;
use crate::terminal_widget::TerminalWidget;

pub struct TerminalContext {
    commands: Commands,
    widgets: Vec<TerminalWidget>,

    input_buffer: Vec<u8>,
    output_buffer: Vec<u8>,
    max_sequences_to_process: u32,
    just_split: bool,

    debug_discrete_processing: bool,
    debug_disable_splits: bool
}

impl TerminalContext {
    pub fn new() -> Self {
        Self {
            commands: Commands::new(),
            widgets: vec![TerminalWidget::new()],

            input_buffer: vec![],
            output_buffer: vec![],
            max_sequences_to_process: 0,
            just_split: false,

            debug_discrete_processing: false,
            debug_disable_splits: false
        }
    }

    pub fn update(&mut self, input: &Input) -> bool {
        let mut any_event = false;

        any_event |= self.handle_user_io(input);
        any_event |= self.handle_process_io();

        any_event
    }

    pub fn get_widgets(&self) -> &Vec<TerminalWidget> { &self.widgets }
    pub fn get_widgets_mut(&mut self) -> &mut Vec<TerminalWidget> { &mut self.widgets }
    pub fn just_split(&self) -> bool { self.just_split }

    fn handle_user_io(&mut self, input: &Input) -> bool {
        self.input_buffer.extend_from_slice(input.get_input_buffer());

        let mut any_event = false;

        if input.get_key_just_pressed(glfw::Key::F1) {
            any_event |= true;
            self.debug_discrete_processing = !self.debug_discrete_processing;
        }

        if input.get_key_just_pressed(glfw::Key::F2) {
            any_event |= true;
            self.debug_disable_splits = !self.debug_disable_splits;
        }

        if input.get_key_just_pressed(glfw::Key::F3) {
            any_event |= true;
            self.widgets.last_mut().as_mut().unwrap().toggle_debug_line_numbers();
        }

        if input.get_key_just_pressed(glfw::Key::F4) {
            any_event |= true;
            self.widgets.last_mut().as_mut().unwrap().toggle_debug_char_numbers();
        }

        if input.get_key_just_pressed(glfw::Key::F6) {
            any_event |= true;
            self.widgets.last_mut().as_mut().unwrap().toggle_debug_show_cursor();
        }

        self.max_sequences_to_process = std::u32::MAX;
        if self.debug_discrete_processing {
            self.max_sequences_to_process = 0;

            if input.get_key_just_pressed(glfw::Key::F10) {
                any_event |= true;
                self.max_sequences_to_process = 1;
            } else if input.get_key_just_pressed(glfw::Key::F11) {
                any_event |= true;
                self.max_sequences_to_process = 10;
            } else if input.get_key_just_pressed(glfw::Key::F12) {
                any_event |= true;
                self.max_sequences_to_process = 100;
            } else if input.get_key_just_pressed(glfw::Key::F5) {
                any_event |= true;
                self.max_sequences_to_process = std::u32::MAX;
            }
        }

        any_event
    }

    fn handle_process_io(&mut self) -> bool {
        let did_split = self.commands.poll_io();
        let output = self.commands.get_output();

        let any_event = self.commands.get_output()[0].len() > 0 || self.commands.get_output()[1].len() > 0;

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

        if did_split && !self.debug_disable_splits {
            let widget_len = self.widgets.len();
            if widget_len > 1 {
                //self.widgets[widget_len - 2].set_expanded(false);
            }

            self.widgets.last_mut().unwrap().set_primary(false);

            self.widgets.push(TerminalWidget::new());
            self.widgets.last_mut().unwrap().push_chars(&output[1], false);
        }
        self.just_split = did_split;

        let last_widget = self.widgets.last_mut().unwrap();
        let last_widget_size = last_widget.get_terminal_size();

        // Next commands should obey the size of the current active widget
        self.commands.resize(last_widget_size[1], last_widget_size[0]);

        // Only need to send input to the active widget
        self.commands.send_input(&last_widget.consume_output_stream());

        self.commands.send_input(&self.input_buffer);
        self.commands.clear_output();

        self.input_buffer.clear();

        any_event
    }
}
