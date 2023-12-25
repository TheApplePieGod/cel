use glfw::{Action, Context, Key, fail_on_errors};

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: glfw::PWindow,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>
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
        window.set_all_polling(true);

        gl::load_with(|s| window.get_proc_address(s) as *const _);

        Self {
            glfw_instance,
            window,
            event_receiver
        }
    }

    pub fn poll_events(&mut self) {
        self.glfw_instance.poll_events();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            match event {
                glfw::WindowEvent::Key(Key::Escape, _, Action::Press, _) => {
                    self.window.set_should_close(true)
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
    }

    pub fn get_handle(&self) -> &glfw::Window { &self.window }
    pub fn get_mut_handle(&mut self) -> &mut glfw::Window { &mut self.window }
    pub fn get_width(&self) -> i32 { self.window.get_size().0 }
    pub fn get_height(&self) -> i32 { self.window.get_size().1 }
    pub fn get_pixel_width(&self) -> i32 { self.window.get_framebuffer_size().0 }
    pub fn get_pixel_height(&self) -> i32 { self.window.get_framebuffer_size().1 }
}
