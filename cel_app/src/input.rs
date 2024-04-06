use glfw::{Action, Key, Modifiers, MouseButton, WindowEvent};
use cli_clipboard::{ClipboardContext, ClipboardProvider};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum PressState {
    #[default]
    Released,
    Pressed,
    JustReleased,
    JustPressed,
    Repeat,
}

pub struct Input {
    clipboard_context: Option<ClipboardContext>,
    input_buffer: Vec<u8>,
    utf8_buffer: [u8; 8],
    key_states: [(PressState, u64); 512],
    mouse_states: [(PressState, u64); 16],
    mouse_pos: [f32; 2],
    mouse_delta: [f32; 2],
    scroll_delta: [f32; 2],
    poll_count: u64,

    // Event flags
    pub event_new_tab: bool,
    pub event_next_tab: bool,
    pub event_prev_tab: bool,
}

impl Input {
    pub fn new() -> Self {
        let clipboard_context = match ClipboardContext::new() {
            Ok(ctx) => Some(ctx),
            Err(err) => {
                log::error!("Failed to initialize clipboard context: {}", err);
                None
            }
        };

        Self {
            clipboard_context,
            input_buffer: vec![],
            utf8_buffer: [0; 8],
            key_states: [Default::default(); 512],
            mouse_states: [Default::default(); 16],
            mouse_pos: [0.0, 0.0],
            mouse_delta: [0.0, 0.0],
            scroll_delta: [0.0, 0.0],
            poll_count: 0,

            event_new_tab: false,
            event_next_tab: false,
            event_prev_tab: false,
        }
    }

    pub fn poll_events(&mut self) {
        self.poll_count += 1;

        self.scroll_delta = [0.0, 0.0];
        self.mouse_delta = [0.0, 0.0];
    }

    // Returns true if the event was handled
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        let og_mouse_pos = self.mouse_pos;

        let handled = match event {
            glfw::WindowEvent::Key(key, _, action, mods) => {
                if Self::handle_input_press(
                    *key as usize,
                    &mut self.key_states,
                    &action,
                    self.poll_count
                ) {
                    #[cfg(target_os = "macos")]
                    let modifier_key = Modifiers::Super;
                    #[cfg(not(target_os = "macos"))]
                    let modifier_key = Modifiers::Control;

                    // Copy/Paste
                    if mods.contains(modifier_key) {
                        match *key {
                            Key::V if self.clipboard_context.is_some() => { // Paste
                                match self.clipboard_context.as_mut().unwrap().get_contents() {
                                    Ok(contents) => self.input_buffer.extend(contents.as_bytes()),
                                    Err(_) => {}
                                }

                                return true
                            },
                            _ => {}
                        }
                    }

                    // Cel commands
                    if mods.contains(Modifiers::Control) {
                        match *key {
                            Key::T => self.event_new_tab = true,
                            Key::Right => self.event_next_tab = true,
                            Key::Left => self.event_prev_tab = true,
                            _ => {}
                        }
                    }

                    self.input_buffer.extend(
                        self.encode_input_key(*key, *mods)
                    );
                }

                true
            },
            glfw::WindowEvent::Char(key) => {
                self.input_buffer.extend_from_slice(
                    key.encode_utf8(&mut self.utf8_buffer).as_bytes()
                );

                true
            },
            glfw::WindowEvent::MouseButton(button, action, mods) => {
                Self::handle_input_press(
                    *button as usize,
                    &mut self.mouse_states,
                    &action,
                    self.poll_count
                );

                true
            },
            glfw::WindowEvent::CursorPos(x, y) => {
                self.mouse_pos = [*x as f32, *y as f32];

                true
            },
            glfw::WindowEvent::Scroll(x, y) => {
                self.scroll_delta[0] += *x as f32;
                self.scroll_delta[1] += *y as f32;

                true
            },
            _ => false,
        };

        self.mouse_delta = [
            self.mouse_pos[0] - og_mouse_pos[0],
            self.mouse_pos[1] - og_mouse_pos[1]
        ];

        handled
    }

    pub fn clear(&mut self) {
        self.input_buffer.clear();
    }

    pub fn get_key_pressed(&self, key: Key) -> bool {
        match self.key_states[key as usize].0 {
            PressState::JustPressed | PressState::Repeat => true,
            _ => false
        }
    }

    pub fn get_key_just_pressed(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        state.0 == PressState::JustPressed && state.1 == self.poll_count
    }

    pub fn get_key_released(&self, key: Key) -> bool {
        self.key_states[key as usize].0 == PressState::JustReleased
    }

    pub fn get_key_just_released(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        state.0 == PressState::JustReleased && state.1 == self.poll_count
    }

    pub fn get_mouse_down(&self, button: MouseButton) -> bool {
        match self.mouse_states[button as usize].0 {
            PressState::JustPressed | PressState::Repeat => true,
            _ => false
        }
    }

    pub fn get_mouse_just_pressed(&self, button: MouseButton) -> bool {
        let state = &self.mouse_states[button as usize];
        state.0 == PressState::JustPressed && state.1 == self.poll_count
    }

    pub fn get_mouse_up(&self, button: MouseButton) -> bool {
        self.mouse_states[button as usize].0 == PressState::JustReleased
    }

    pub fn get_mouse_just_released(&self, button: MouseButton) -> bool {
        let state = &self.mouse_states[button as usize];
        state.0 == PressState::JustReleased && state.1 == self.poll_count
    }

    pub fn is_super_down(&self) -> bool { self.get_key_pressed(Key::LeftSuper) || self.get_key_pressed(Key::RightSuper) }
    pub fn is_shift_down(&self) -> bool { self.get_key_pressed(Key::LeftShift) || self.get_key_pressed(Key::RightShift) }
    pub fn is_ctrl_down(&self) -> bool { self.get_key_pressed(Key::LeftControl) || self.get_key_pressed(Key::RightControl) }
    pub fn is_alt_down(&self) -> bool { self.get_key_pressed(Key::LeftAlt) || self.get_key_pressed(Key::RightAlt) }
    pub fn get_input_buffer(&self) -> &Vec<u8> { &self.input_buffer }
    pub fn get_mouse_pos(&self) -> [f32; 2] { self.mouse_pos }
    pub fn get_mouse_delta(&self) -> [f32; 2] { self.mouse_delta }
    pub fn get_scroll_delta(&self) -> [f32; 2] { self.scroll_delta }

    // Returns true if the input was a press and the input buffer should be extended
    fn handle_input_press(
        id: usize,
        states: &mut [(PressState, u64)],
        action: &Action,
        poll_count: u64
    ) -> bool {
        let mut press = false;
        if id < states.len() {
            let new_state;
            match action {
                Action::Press | Action::Repeat => {
                    new_state = match action {
                        Action::Repeat => PressState::Repeat,
                        _ => PressState::JustPressed,
                    };

                    press = true;
                },
                Action::Release => new_state = PressState::JustReleased
            };
            states[id] = (new_state, poll_count);
        }

        press
    }

    fn glfw_key_to_ascii(&self, key: Key) -> Option<u8> {
        let val = key as i32;
        if val >= 32 && val <= 126 {
            Some(val as u8)
        } else {
            None
        }
    }

    // Convert an ascii character to its relative control character (when ctrl is down)
    fn ascii_to_control(&self, key: u8) -> u8 {
        match key {
            b' '  => 0,
            b'/'  => 31,
            b'0'  => 48,
            b'1'  => 49,
            b'2'  => 0,
            b'3'  => 27,
            b'4'  => 28,
            b'5'  => 29,
            b'6'  => 30,
            b'7'  => 31,
            b'8'  => 127,
            b'9'  => 57,
            b'?'  => 127,
            b'@'  => 0,
            b'['  => 27,
            b'\\' => 28,
            b']'  => 29,
            b'^'  => 30,
            b'_'  => 31,
            b'~'  => 30,
            b'A'..=b'Z' => key - 64,
            _ => key
        }
    }

    // https://github.com/kovidgoyal/kitty/blob/master/kitty/key_encoding.c#L148
    // http://www.leonerd.org.uk/hacks/fixterms/
    fn encode_input_key(&self, key: Key, mods: Modifiers) -> Vec<u8> {
        let mut result = vec![];

        // TODO: Keypad keys
        // TODO: https://stackoverflow.com/questions/12382499/looking-for-altleftarrowkey-solution-in-zsh
        // TODO: https://github.com/kovidgoyal/kitty/issues/838

        match self.glfw_key_to_ascii(key) {
            Some(mut k) => { // Printable
                // Do not handle raw characters since the char callback does this 
                if !mods.is_empty() && mods != Modifiers::Shift {
                    if mods.contains(Modifiers::Alt) {
                        result.push(0x1b);
                    }
                    if mods.contains(Modifiers::Control) {
                        k = self.ascii_to_control(k);
                    }
                    result.push(k);
                }
            },
            None => 'handled: { // Function character
                let esc_char = match key {
                    Key::Up => b'A',
                    Key::Down => b'B',
                    Key::Right => b'C',
                    Key::Left => b'D',
                    Key::End => b'F',
                    Key::Home => b'H',
                    Key::F1 => b'P',
                    Key::F2 => b'Q',
                    Key::F3 => b'R',
                    Key::F4 => b'S',
                    _ => 0
                };
                if esc_char > 0 {
                    result.extend_from_slice(&[0x1b, b'O', esc_char]);
                    break 'handled;
                }

                let esc_char = match key {
                    Key::Tab => b'\t',
                    Key::Enter => b'\r',
                    Key::Escape => 0x1b,
                    Key::Backspace => 0x7f, // 0x08
                    Key::Delete => 0x7f,
                    _ => 0
                };
                if esc_char > 0 {
                    if mods.contains(Modifiers::Alt) {
                        result.push(0x1b);
                    }
                    result.push(esc_char);
                    break 'handled;
                }
            }
        }

        result
    }
}
