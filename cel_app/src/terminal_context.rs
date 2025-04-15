use cel_core::commands::Commands;
use cli_clipboard::{ClipboardContext, ClipboardProvider};

use crate::input::{Input, InputEvent};
use crate::terminal_widget::TerminalWidget;

pub struct TerminalContext {
    commands: Commands,
    widgets: Vec<TerminalWidget>,

    input_buffer: Vec<u8>,
    output_buffer: Vec<u8>,
    max_sequences_to_process: u32,
    just_split: bool,

    debug_discrete_processing: bool,
    debug_disable_splits: bool,
    debug_disable_input: bool
}

impl TerminalContext {
    pub fn new(max_rows: u32, max_cols: u32, cwd: Option<&str>) -> Self {
        Self {
            commands: Commands::new(max_rows, max_cols, cwd),
            widgets: vec![TerminalWidget::new(max_rows, max_cols)],

            input_buffer: vec![],
            output_buffer: vec![],
            max_sequences_to_process: 0,
            just_split: false,

            debug_discrete_processing: false,
            debug_disable_splits: false,
            debug_disable_input: false
        }
    }

    // Returns (any_event, terminated)
    pub fn update(&mut self, input: Option<&mut Input>) -> (bool, bool) {
        let mut any_event = false;

        self.max_sequences_to_process = std::u32::MAX;

        if let Some(input) = input {
            any_event |= input.consume_event(InputEvent::Paste, || {
                match ClipboardContext::new() {
                    Ok(mut ctx) => {
                        let text = ctx.get_contents().unwrap_or(String::new());
                        match self.widgets.last_mut().unwrap().is_bracketed_paste_enabled() {
                            true => {
                                let bracketed = format!("\x1b[200~{}\x1b[201~", text);
                                self.input_buffer.extend_from_slice(bracketed.as_bytes())
                            },
                            false => self.input_buffer.extend_from_slice(text.as_bytes()),
                        };
                    }
                    Err(err) => {
                        log::error!("Failed to initialize clipboard context: {}", err);
                    }
                };
            });

            any_event |= self.handle_user_io(input);
        }

        let (pio_event, terminated) = self.handle_process_io();
        any_event |= pio_event;

        (any_event, terminated)
    }

    pub fn resize(&mut self, max_rows: u32, max_cols: u32) {
        self.get_primary_widget_mut().resize(max_rows, max_cols);
        self.commands.resize(max_rows, max_cols);
    }

    pub fn get_size(&self) -> [u32; 2] { self.commands.get_size() }
    pub fn get_primary_widget(&self) -> &TerminalWidget { self.widgets.last().unwrap() }
    pub fn get_primary_widget_mut(&mut self) -> &mut TerminalWidget { self.widgets.last_mut().unwrap() }
    pub fn get_widgets(&self) -> &Vec<TerminalWidget> { &self.widgets }
    pub fn get_widgets_mut(&mut self) -> &mut Vec<TerminalWidget> { &mut self.widgets }
    pub fn just_split(&self) -> bool { self.just_split }

    fn handle_user_io(&mut self, input: &Input) -> bool {
        if !self.debug_disable_input {
            self.input_buffer.extend_from_slice(input.get_input_buffer());
        }

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

        if input.get_key_just_pressed(glfw::Key::F7) {
            any_event |= true;
            self.debug_disable_input = !self.debug_disable_input;
        }

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

    // Returns (any_event, terminated)
    fn handle_process_io(&mut self) -> (bool, bool) {
        let terminated = self.commands.poll_io();

        let output = self.commands.get_output();
        let any_event = self.commands.get_output().len() > 0;

        self.just_split = false;
        self.output_buffer.extend_from_slice(&output);
        for _ in 0..self.max_sequences_to_process {
            match self.widgets.last_mut().unwrap().push_chars(
                &self.output_buffer,
                self.debug_discrete_processing
            ) {
                Some((i, split)) => {
                    self.output_buffer.drain(0..=(i as usize));
                    if split && !self.debug_disable_splits {
                        // Primary widget is always the last one
                        let size = self.commands.get_size();
                        self.widgets.last_mut().unwrap().set_primary(false);
                        self.widgets.push(TerminalWidget::new(size[0], size[1]));
                        self.just_split = true;
                    }
                    self.just_split = split;
                },
                None => {
                    self.output_buffer.clear();
                    break;
                }
            }
        }

        // Only need to send input to the active widget
        let last_widget = self.widgets.last_mut().unwrap();
        self.commands.send_input(&last_widget.consume_output_stream());

        self.commands.send_input(&self.input_buffer);
        self.commands.clear_output();

        self.input_buffer.clear();

        (any_event, terminated)
    }
}
