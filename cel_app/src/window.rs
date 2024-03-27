
use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

use cel_renderer::renderer::Renderer;
use glfw::{Context, fail_on_errors};

use crate::app_state::AppState;
use crate::layout::Layout;
use crate::input::Input;

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: Rc<RefCell<glfw::PWindow>>,
    renderer: Rc<RefCell<Renderer>>,
    layout: Rc<RefCell<Layout>>,
    input: Input,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    background_color: [f32; 3],
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
        window.set_cursor_pos_polling(true);
        window.set_scroll_polling(true);
        window.set_resizable(true);
        
        gl::load_with(|s| window.get_proc_address(s) as *const _);

        // Disable vsync to decrease latency
        glfw_instance.set_swap_interval(glfw::SwapInterval::None);

        let initial_size_px = window.get_framebuffer_size();
        Self {
            glfw_instance,
            window: Rc::new(RefCell::new(window)),
            renderer: Rc::new(RefCell::new(Renderer::new(
                initial_size_px.0,
                initial_size_px.1,
                AppState::current().as_ref().borrow().font.clone()
            ))),
            layout: Rc::new(RefCell::new(Layout::new())),
            input: Input::new(),
            event_receiver,
            background_color: [0.0, 0.0, 0.0],
        }
    }

    pub fn update(&mut self) {
        let renderer_ptr = self.renderer.clone();
        let layout_ptr = self.layout.clone();
        let window_ptr = self.window.clone();
        let clear_color = self.background_color;
        self.window.as_ref().borrow_mut().set_refresh_callback(move |w| {
            // Update
            Self::on_resized_wrapper(
                w.get_size().into(),
                w.get_framebuffer_size().into(),
                renderer_ptr.as_ref().borrow_mut().deref_mut()
            );

            // Glitchy
            //let dummy_input = Input::new();
            //layout_ptr.as_ref().borrow_mut().update(&dummy_input);

            // Render
            Self::render_wrapper(
                &clear_color,
                renderer_ptr.as_ref().borrow_mut().deref_mut(),
                layout_ptr.as_ref().borrow_mut().deref_mut(),
                window_ptr.as_ref().borrow_mut().deref_mut()
            );
        });
        self.poll_events();

        self.layout.as_ref().borrow_mut().update(&self.input);

        self.input.clear();
    }

    pub fn render(&mut self) {
        Self::render_wrapper(
            &self.background_color,
            self.renderer.as_ref().borrow_mut().deref_mut(),
            self.layout.as_ref().borrow_mut().deref_mut(),
            self.window.as_ref().borrow_mut().deref_mut(),
        );
    }

    pub fn should_close(&self) -> bool { self.window.as_ref().borrow().should_close() }
    pub fn get_input(&self) -> &Input { &self.input }
    pub fn get_width(&self) -> i32 { self.window.as_ref().borrow().get_size().0 }
    pub fn get_height(&self) -> i32 { self.window.as_ref().borrow().get_size().1 }
    pub fn get_size(&self) -> [i32; 2] { self.window.as_ref().borrow().get_size().into() }
    pub fn get_pixel_width(&self) -> i32 { self.window.as_ref().borrow().get_framebuffer_size().0 }
    pub fn get_pixel_height(&self) -> i32 { self.window.as_ref().borrow().get_framebuffer_size().1 }
    pub fn get_pixel_size(&self) -> [i32; 2] { self.window.as_ref().borrow().get_framebuffer_size().into() }
    pub fn get_time_seconds(&self) -> f64 { self.glfw_instance.get_time() }

    fn poll_events(&mut self) {
        let mut resize = false;

        self.input.poll_events();

        self.glfw_instance.poll_events();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            if self.input.handle_window_event(&event) {
                continue;
            }

            match event {
                glfw::WindowEvent::Size(_, _) => {
                    resize = true;
                },
                _ => {},
            }
        }

        if resize {
            Self::on_resized_wrapper(
                self.get_size(),
                self.get_pixel_size(),
                self.renderer.as_ref().borrow_mut().deref_mut()
            )
        }
    }

    fn render_wrapper(
        clear_color: &[f32; 3],
        renderer: &mut Renderer,
        layout: &mut Layout,
        window: &mut glfw::PWindow
    ) {
        Self::begin_frame(clear_color);
        layout.render(renderer);
        Self::end_frame(window);
    }

    fn on_resized_wrapper(
        new_size: [i32; 2],
        new_pixel_size: [i32; 2],
        renderer: &mut Renderer
    ) {
        renderer.update_viewport_size(
            new_pixel_size[0],
            new_pixel_size[1],
        );
    }

    fn begin_frame<'a>(clear_color: &[f32; 3]) {
        unsafe {
            gl::ClearColor(
                clear_color[0],
                clear_color[1],
                clear_color[2],
                0.0
            );
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }
    }

    fn end_frame(window: &mut glfw::PWindow) {
        window.swap_buffers();
    }
}
