use glfw::{Action, Context, Key, fail_on_errors};

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: glfw::PWindow,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    key_states: [bool; 512],
    input_buffer: Vec<u8>,
    utf8_buffer: [u8; 8]
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
            1920, 1080, "cel_", 
            glfw::WindowMode::Windowed
        ).expect("Failed to create GLFW window.");

        window.make_current();
        window.set_key_polling(true);
        window.set_char_polling(true);
        window.set_char_mods_polling(true);

        gl::load_with(|s| window.get_proc_address(s) as *const _);

        // Disable vsync to decrease latency
        glfw_instance.set_swap_interval(glfw::SwapInterval::None);

        Self {
            glfw_instance,
            window,
            event_receiver,
            key_states: [false; 512],
            input_buffer: vec![],
            utf8_buffer: [0; 8]
        }
    }

    pub fn poll_events(&mut self) {
        self.glfw_instance.poll_events();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            match event {
                glfw::WindowEvent::Key(key, _, action, _) => {
                    if (key as usize) < self.key_states.len() {
                        let key_state;
                        match action {
                            Action::Press | Action::Repeat => {
                                key_state = true;

                                // Send escape code to input buffer
                                let ctrl_pressed = self.get_key_pressed(Key::LeftControl) || self.get_key_pressed(Key::RightControl);
                                match key {
                                    Key::Up => self.input_buffer.extend_from_slice(&[0x1b, b'[', b'A']),
                                    Key::Down => self.input_buffer.extend_from_slice(&[0x1b, b'[', b'B']),
                                    Key::Right => self.input_buffer.extend_from_slice(&[0x1b, b'[', b'C']),
                                    Key::Left => self.input_buffer.extend_from_slice(&[0x1b, b'[', b'D']),
                                    Key::Backspace => self.input_buffer.push(0x08),
                                    Key::Delete => self.input_buffer.push(0x7F),
                                    Key::Tab => self.input_buffer.push(0x09),
                                    Key::Escape => self.input_buffer.push(0x1b),
                                    Key::Enter => self.input_buffer.push(0x0A),
                                    Key::C if ctrl_pressed => self.input_buffer.push(0x03),
                                    _ => {}
                                }
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

    pub fn begin_frame<'a>(&mut self) {
        unsafe {
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
    pub fn get_input_buffer(&self) -> &Vec<u8> { &self.input_buffer }
}
