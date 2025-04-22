use enum_map::EnumMap;
use glfw::{Action, Key, Modifiers, MouseButton, WindowEvent};

#[derive(Default, Copy, Clone, PartialEq, Eq)]
pub enum PressState {
    #[default]
    Released,
    Pressed,
    JustReleased,
    JustPressed,
    Repeat,
}

#[derive(Clone, Copy, enum_map::Enum)]
pub enum InputEvent {
    ZoomReset,
    ZoomIn,
    ZoomOut,
    TabNew,
    TabDelete,
    TabNext,
    TabPrev,
    TabMoveLeft,
    TabMoveRight,
    Copy,
    Paste,
}

pub struct Input {
    input_buffer: Vec<u8>,
    utf8_buffer: [u8; 8],
    key_states: [(PressState, u64); 512],
    mouse_states: [(PressState, u64); 16],
    mouse_pos: [f32; 2],
    mouse_delta: [f32; 2],
    scroll_delta: [f32; 2],
    poll_count: u64,
    event_flags: EnumMap<InputEvent, bool>,
}

impl Input {
    pub fn new() -> Self {
        Self {
            input_buffer: vec![],
            utf8_buffer: [0; 8],
            key_states: [Default::default(); 512],
            mouse_states: [Default::default(); 16],
            mouse_pos: [0.0, 0.0],
            mouse_delta: [0.0, 0.0],
            scroll_delta: [0.0, 0.0],
            poll_count: 0,
            event_flags: EnumMap::default(),
        }
    }

    pub fn poll_events(&mut self) {
        self.poll_count += 1;

        self.scroll_delta = [0.0, 0.0];
        self.mouse_delta = [0.0, 0.0];
    }

    pub fn reset_input_state(&mut self) {
        self.key_states = [Default::default(); 512];
        self.mouse_states = [Default::default(); 16];
        self.event_flags = EnumMap::default();
    }

    // Returns true if the event was handled
    pub fn handle_window_event(&mut self, event: &WindowEvent) -> bool {
        let og_mouse_pos = self.mouse_pos;

        //log::warn!("{:?}", event);
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
                    let modifier_key = Modifiers::Control | Modifiers::Shift;

                    // Copy/Paste
                    if mods.contains(modifier_key) {
                        let mut handled = true;
                        match *key {
                            Key::C => self.set_event(InputEvent::Copy),
                            Key::V => self.set_event(InputEvent::Paste),
                            _ => handled = false
                        }

                        if handled {
                            return true;
                        }
                    }

                    // Cel commands
                    if mods.contains(modifier_key | Modifiers::Shift) {
                        let mut handled = true;
                        match *key {
                            Key::Num0 => self.set_event(InputEvent::ZoomReset),
                            Key::Equal => self.set_event(InputEvent::ZoomIn),
                            Key::Minus => self.set_event(InputEvent::ZoomOut),
                            Key::T => self.set_event(InputEvent::TabNew),
                            Key::W => self.set_event(InputEvent::TabDelete),
                            Key::Left => self.set_event(InputEvent::TabPrev),
                            Key::Right => self.set_event(InputEvent::TabNext),
                            Key::Comma => self.set_event(InputEvent::TabMoveLeft),
                            Key::Period => self.set_event(InputEvent::TabMoveRight),
                            _ => handled = false
                        }

                        if handled {
                            return true;
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
            glfw::WindowEvent::Pos(_, _) |
            glfw::WindowEvent::Size(_, _) |
            glfw::WindowEvent::Focus(_) => {
                // These events could cause inconsistent input state (i.e. a mouse
                // up event is never sent). Thus, we reset to ensure consistency
                self.reset_input_state();

                false
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

    // Returns true if the event was consumed
    pub fn consume_event(&mut self, event: InputEvent, mut fun: impl FnMut()) -> bool {
        if self.event_flags[event] {
            self.event_flags[event] = false;
            fun();
            true
        } else {
            false
        }
    }

    // Returns true if the event was consumed
    pub fn maybe_consume_event(&mut self, event: InputEvent, mut fun: impl FnMut() -> bool) -> bool {
        if self.event_flags[event] {
            if fun() {
                self.event_flags[event] = false;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn get_key_down(&self, key: Key) -> bool {
        match self.key_states[key as usize].0 {
            PressState::JustPressed | PressState::Repeat => true,
            _ => false
        }
    }

    pub fn get_key_just_pressed(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        state.0 == PressState::JustPressed && state.1 == self.poll_count
    }

    // Same as just pressed, but also returns true on each repeat
    pub fn get_key_triggered(&self, key: Key) -> bool {
        let state = &self.key_states[key as usize];
        (state.0 == PressState::JustPressed || state.0 == PressState::Repeat) && state.1 == self.poll_count
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

    pub fn is_super_down(&self) -> bool { self.get_key_down(Key::LeftSuper) || self.get_key_down(Key::RightSuper) }
    pub fn is_shift_down(&self) -> bool { self.get_key_down(Key::LeftShift) || self.get_key_down(Key::RightShift) }
    pub fn is_ctrl_down(&self) -> bool { self.get_key_down(Key::LeftControl) || self.get_key_down(Key::RightControl) }
    pub fn is_alt_down(&self) -> bool { self.get_key_down(Key::LeftAlt) || self.get_key_down(Key::RightAlt) }
    pub fn get_input_buffer(&self) -> &Vec<u8> { &self.input_buffer }
    pub fn get_mouse_pos(&self) -> [f32; 2] { self.mouse_pos }
    pub fn get_mouse_delta(&self) -> [f32; 2] { self.mouse_delta }
    pub fn get_scroll_delta(&self) -> [f32; 2] { self.scroll_delta }

    fn set_event(&mut self, event: InputEvent) {
        self.event_flags[event] = true;
    }

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
    fn ascii_to_control(&self, key: u8) -> Option<u8> {
        // Skip some problematic characters (commented)
        match key {
            b' '  => Some(0),
            b'/'  => Some(31),
            b'0'  => Some(48),
            b'1'  => Some(49),
            b'2'  => Some(0),
            b'3'  => Some(27),
            b'4'  => Some(28),
            b'5'  => Some(29),
            b'6'  => Some(30),
            b'7'  => Some(31),
            b'8'  => Some(127),
            b'9'  => Some(57),
            b'?'  => Some(127),
            //b'@'  => Some(0),
            //b'['  => Some(27),
            b'\\' => Some(28),
            b']'  => Some(29),
            b'^'  => Some(30),
            b'_'  => Some(31),
            b'~'  => Some(30),
            b'a'..=b'z' if key != b'm' && key != b'i'
                => Some(key - 96),
            _ => None
        }
    }

    fn serialize_input_key(&self, c: u8, trailer: char, mods: Modifiers) -> Vec<u8> {
        if mods.is_empty() {
            return vec![c]
        }

        let encode = |mods: i32| format!("\x1b[{};{}{}", c, mods, trailer).into_bytes();

        let mut result = vec![];
        let mut mods_int = 1;
        let unicode_trailer = trailer == 'u';
        if unicode_trailer {
            if mods.contains(Modifiers::Shift) {
                mods_int += 1;
            }
            if mods.contains(Modifiers::Alt) {
                mods_int += 2;
                result.push(0x1b);
            }
            if mods.contains(Modifiers::Control) {
                mods_int += 4;
                match self.ascii_to_control(c) {
                    // Attempt to map to legacy control, fallback to encoding if necessary
                    Some(ctrl) => result.push(ctrl),
                    None => result = encode(mods_int)
                }
            } else {
                result.push(c);
            }
        } else {
            if mods.contains(Modifiers::Shift) {
                mods_int += 1;
            }
            if mods.contains(Modifiers::Alt) {
                mods_int += 2;
            }
            if mods.contains(Modifiers::Control) {
                mods_int += 4;
            }

            result = encode(mods_int);
        }


        result
    }

    // https://github.com/kovidgoyal/kitty/blob/master/kitty/key_encoding.c#L148
    // http://www.leonerd.org.uk/hacks/fixterms/
    fn encode_input_key(&self, key: Key, mods: Modifiers) -> Vec<u8> {
        // TODO: Keypad keys
        // TODO: https://stackoverflow.com/questions/12382499/looking-for-altleftarrowkey-solution-in-zsh
        // TODO: https://github.com/kovidgoyal/kitty/issues/838

        let result = match self.glfw_key_to_ascii(key) { 
            Some(mut k) => { // Printable
                // Do not handle raw characters nor alt chars since the char callback does this 
                let modified = !mods.is_empty() && mods != Modifiers::Shift;
                let is_alt = mods == Modifiers::Alt && !k.is_ascii_control();
                if modified && !is_alt {
                    let mut adjusted_mods = mods.clone();
                    if k >= b'A' && k <= b'Z' {
                        if mods.contains(Modifiers::Shift) {
                            // Shift already handled
                            adjusted_mods.set(Modifiers::Shift, false);
                        } else {
                            // Shift to lowercase
                            k += 32;
                        }
                    }

                    self.serialize_input_key(k, 'u', adjusted_mods)
                } else {
                    vec![]
                }
            },
            None => { // Function character
                let (esc_char, trailer) = match key {
                    Key::Up if !mods.is_empty() => (1, 'A'),
                    Key::Down if !mods.is_empty() => (1, 'B'),
                    Key::Right if !mods.is_empty() => (1, 'C'),
                    Key::Left if !mods.is_empty() => (1, 'D'),
                    Key::End => (1, 'F'),
                    Key::Home => (1, 'H'),
                    Key::Tab => (b'\t', 'u'),
                    Key::Enter => (b'\r', 'u'),
                    Key::Escape => (0x1b, 'u'),
                    Key::Backspace => (0x7f, 'u'), // 0x08
                    Key::Delete => (0x7f, 'u'),
                    Key::F1 => (1, 'P'),
                    Key::F2 => (1, 'Q'),
                    Key::F3 => (1, 'R'),
                    Key::F4 => (1, 'S'),
                    // TODO: more function keys
                    _ => (0, 'u')
                };

                if esc_char == 0 {
                    vec![]
                } else if mods.is_empty() {
                    match trailer {
                        'u' => vec![ esc_char ],
                        _ => vec![ 0x1b, b'[', trailer as u8 ]
                    }
                } else {
                    self.serialize_input_key(esc_char, trailer, mods)
                }
            }
        };

        result
    }
}
