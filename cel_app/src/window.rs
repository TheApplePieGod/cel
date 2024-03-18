use std::{sync::mpsc, thread};
use glfw::{Action, Context, Key, fail_on_errors, Modifiers};

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: glfw::PWindow,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    key_states: [bool; 512],
    input_buffer: Vec<u8>,
    utf8_buffer: [u8; 8],
    just_resized: bool
}

impl Window {
    pub fn new() -> Self {
        let mut glfw_instance = glfw::init(fail_on_errors!()).unwrap();

        glfw_instance.window_hint(glfw::WindowHint::ContextVersion(4, 0));
        glfw_instance.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));
        glfw_instance.window_hint(glfw::WindowHint::OpenGlProfile(
            glfw::OpenGlProfileHint::Core
        ));

        let (mut window, event_receiver) = glfw_instance.create_window(
            1920, 600, "cel_", 
            glfw::WindowMode::Windowed
        ).expect("Failed to create GLFW window.");

        window.make_current();
        window.set_size_polling(true);
        window.set_key_polling(true);
        window.set_char_polling(true);
        window.set_char_mods_polling(true);
        window.set_resizable(true);

        gl::load_with(|s| window.get_proc_address(s) as *const _);

        // Disable vsync to decrease latency
        glfw_instance.set_swap_interval(glfw::SwapInterval::None);

        Self {
            glfw_instance,
            window,
            event_receiver,
            key_states: [false; 512],
            input_buffer: vec![],
            utf8_buffer: [0; 8],
            just_resized: false
        }
    }

    // https://en.wikipedia.org/wiki/ANSI_escape_code#Terminal_input_sequences
    pub fn poll_events(&mut self) {
        self.just_resized = false;

        self.glfw_instance.poll_events();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            match event {
                glfw::WindowEvent::Size(_, _) => {
                    self.just_resized = true;
                },
                glfw::WindowEvent::Key(key, _, action, mods) => {
                    if (key as usize) < self.key_states.len() {
                        let key_state;
                        match action {
                            Action::Press | Action::Repeat => {
                                key_state = true;
                                self.input_buffer.extend(
                                    self.encode_input_key(key, mods)
                                );
                            },
                            Action::Release => key_state = false
                        };
                        self.key_states[key as usize] = key_state;
                    }
                },
                glfw::WindowEvent::Char(key) => {
                    self.input_buffer.extend_from_slice(
                        key.encode_utf8(&mut self.utf8_buffer).as_bytes()
                    );
                },
                _ => {},
            }
        }
    }

    pub fn begin_frame<'a>(&mut self, color: &[f32; 3]) {
        unsafe {
            gl::ClearColor(color[0], color[1], color[2], 0.0);
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }
    }

    pub fn end_frame(&mut self) {
        self.window.swap_buffers();
        self.input_buffer.clear();
    }

    pub fn get_handle(&self) -> &glfw::Window { &self.window }
    pub fn get_mut_handle(&mut self) -> &mut glfw::Window { &mut self.window }
    pub fn get_width(&self) -> i32 { self.window.get_size().0 }
    pub fn get_height(&self) -> i32 { self.window.get_size().1 }
    pub fn get_pixel_width(&self) -> i32 { self.window.get_framebuffer_size().0 }
    pub fn get_pixel_height(&self) -> i32 { self.window.get_framebuffer_size().1 }
    pub fn get_key_pressed(&self, key: Key) -> bool { self.key_states[key as usize] }
    pub fn is_super_down(&self) -> bool { self.get_key_pressed(Key::LeftSuper) || self.get_key_pressed(Key::RightSuper) }
    pub fn is_shift_down(&self) -> bool { self.get_key_pressed(Key::LeftShift) || self.get_key_pressed(Key::RightShift) }
    pub fn is_ctrl_down(&self) -> bool { self.get_key_pressed(Key::LeftControl) || self.get_key_pressed(Key::RightControl) }
    pub fn is_alt_down(&self) -> bool { self.get_key_pressed(Key::LeftAlt) || self.get_key_pressed(Key::RightAlt) }
    pub fn get_input_buffer(&self) -> &Vec<u8> { &self.input_buffer }
    pub fn was_resized(&self) -> bool { self.just_resized }

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
                    Key::Backspace => 0x08,
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
