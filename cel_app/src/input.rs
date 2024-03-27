use glfw::{Action, Key, Modifiers, WindowEvent};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum KeyPressState {
    #[default]
    Released,
    Pressed,
    JustReleased,
    JustPressed,
    Repeat,
}

pub struct Input {
    input_buffer: Vec<u8>,
    utf8_buffer: [u8; 8],
    key_states: [(KeyPressState, u64); 512],
    poll_count: u64
}

impl Input {
    pub fn new() -> Self {
        Self {
            input_buffer: vec![],
            utf8_buffer: [0; 8],
            key_states: [Default::default(); 512],
            poll_count: 0
        }
    }

    pub fn poll_events(&mut self) {
        self.poll_count += 1;
    }

    // Returns true if the event was handled
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        match event {
            glfw::WindowEvent::Key(key, _, action, mods) => {
                if (*key as usize) < self.key_states.len() {
                    let key_state;
                    match action {
                        Action::Press | Action::Repeat => {
                            key_state = match action {
                                Action::Repeat => KeyPressState::Repeat,
                                _ => KeyPressState::JustPressed,
                            };
                            self.input_buffer.extend(
                                self.encode_input_key(*key, *mods)
                            );
                        },
                        Action::Release => key_state = KeyPressState::JustReleased
                    };
                    self.key_states[*key as usize] = (key_state, self.poll_count);
                }

                true
            },
            glfw::WindowEvent::Char(key) => {
                self.input_buffer.extend_from_slice(
                    key.encode_utf8(&mut self.utf8_buffer).as_bytes()
                );

                true
            },
            _ => false,
        }
    }

    pub fn clear(&mut self) {
        self.input_buffer.clear();
    }

    pub fn get_key_pressed(&self, key: Key) -> bool {
        match self.key_states[key as usize].0 {
            KeyPressState::JustPressed | KeyPressState::Repeat => true,
            _ => false
        }
    }

    pub fn get_key_just_pressed(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        state.0 == KeyPressState::JustPressed && state.1 == self.poll_count
    }

    pub fn get_key_released(&self, key: Key) -> bool {
        self.key_states[key as usize].0 == KeyPressState::JustReleased
    }

    pub fn get_key_just_just_released(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        state.0 == KeyPressState::JustReleased && state.1 == self.poll_count
    }

    pub fn is_super_down(&self) -> bool { self.get_key_pressed(Key::LeftSuper) || self.get_key_pressed(Key::RightSuper) }
    pub fn is_shift_down(&self) -> bool { self.get_key_pressed(Key::LeftShift) || self.get_key_pressed(Key::RightShift) }
    pub fn is_ctrl_down(&self) -> bool { self.get_key_pressed(Key::LeftControl) || self.get_key_pressed(Key::RightControl) }
    pub fn is_alt_down(&self) -> bool { self.get_key_pressed(Key::LeftAlt) || self.get_key_pressed(Key::RightAlt) }
    pub fn get_input_buffer(&self) -> &Vec<u8> { &self.input_buffer }

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
