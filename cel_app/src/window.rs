
use std::borrow::BorrowMut;
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::time::Instant;

use cel_renderer::renderer::Renderer;
use glfw::{Context, fail_on_errors};

use crate::app_state::AppState;
use crate::layout::Layout;
use crate::input::Input;

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: Rc<RefCell<glfw::PWindow>>,
    renderer: Rc<RefCell<Renderer>>,
    layouts: Rc<RefCell<Vec<Layout>>>,
    input: Rc<RefCell<Input>>,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    background_color: [f32; 3],
    last_update_time: Instant,
    active_layout_idx: usize
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
        window.set_mouse_button_polling(true);
        window.set_cursor_pos_polling(true);
        window.set_scroll_polling(true);
        window.set_content_scale_polling(true);
        window.set_resizable(true);
        
        gl::load_with(|s| window.get_proc_address(s) as *const _);

        // Disable vsync to decrease latency
        glfw_instance.set_swap_interval(glfw::SwapInterval::None);

        let scale = window.get_content_scale();
        let initial_size_px = window.get_size();
        Self {
            glfw_instance,
            window: Rc::new(RefCell::new(window)),
            renderer: Rc::new(RefCell::new(Renderer::new(
                initial_size_px.0,
                initial_size_px.1,
                scale.into(),
                AppState::current().as_ref().borrow().font.clone()
            ))),
            layouts: Rc::new(RefCell::new(vec![Layout::new(
                initial_size_px.0,
                initial_size_px.1
            )])),
            input: Rc::new(RefCell::new(Input::new())),
            event_receiver,
            background_color: [0.05, 0.05, 0.1],
            //background_color: [0.0, 0.0, 0.0],
            //background_color: [1.0, 0.0, 0.0],
            last_update_time: Instant::now(),
            active_layout_idx: 0
        }
    }

    pub fn update_and_render(&mut self) {
        let renderer_ptr = self.renderer.clone();
        let layout_ptr = self.layouts.clone();
        let window_ptr = self.window.clone();
        let input_ptr = self.input.clone();
        let clear_color = self.background_color;
        let active_layout_idx = self.active_layout_idx;
        self.window.as_ref().borrow_mut().set_refresh_callback(move |w| {
            let mut layouts = layout_ptr.as_ref().borrow_mut();

            // Update
            Self::on_resized_wrapper(
                w.get_size().into(),
                renderer_ptr.as_ref().borrow_mut().deref_mut(),
                &mut layouts
            );

            // !Glitchy!
            /*
            layout_ptr.as_ref().borrow_mut().update(
                input_ptr.as_ref().borrow().deref()
            );
            */

            // Render
            let active_layout = &mut layouts[active_layout_idx];
            Self::render_wrapper(
                &clear_color,
                renderer_ptr.as_ref().borrow_mut().deref_mut(),
                active_layout,
                window_ptr.as_ref().borrow_mut().deref_mut(),
                input_ptr.as_ref().borrow().deref()
            );
        });

        let mut any_event = false;

        any_event |= self.poll_events();

        // Update layout
        let active_layout = &mut self.layouts.as_ref().borrow_mut()[self.active_layout_idx];
        any_event |= active_layout.update(
            self.input.as_ref().borrow().deref()
        );

        if any_event {
            self.last_update_time = Instant::now();
        }

        // Render
        let time_dist = (Instant::now() - self.last_update_time).as_secs_f32();
        let recently_updated = time_dist <= 3.0;
        if recently_updated {
            Self::render_wrapper(
                &self.background_color,
                self.renderer.as_ref().borrow_mut().deref_mut(),
                active_layout,
                self.window.as_ref().borrow_mut().deref_mut(),
                self.input.as_ref().borrow().deref()
            );
        } else {
            std::thread::sleep(std::time::Duration::new(0, 50e6 as u32));
        }

        self.input.as_ref().borrow_mut().clear();
    }

    pub fn should_close(&self) -> bool { self.window.as_ref().borrow().should_close() }
    pub fn get_width(&self) -> i32 { self.window.as_ref().borrow().get_size().0 }
    pub fn get_height(&self) -> i32 { self.window.as_ref().borrow().get_size().1 }
    pub fn get_size(&self) -> [i32; 2] { self.window.as_ref().borrow().get_size().into() }
    pub fn get_pixel_width(&self) -> i32 { self.window.as_ref().borrow().get_framebuffer_size().0 }
    pub fn get_pixel_height(&self) -> i32 { self.window.as_ref().borrow().get_framebuffer_size().1 }
    pub fn get_pixel_size(&self) -> [i32; 2] { self.window.as_ref().borrow().get_framebuffer_size().into() }
    pub fn get_scale(&self) -> [f32; 2] { self.window.as_ref().borrow().get_content_scale().into() }
    pub fn get_time_seconds(&self) -> f64 { self.glfw_instance.get_time() }

    fn poll_events(&mut self) -> bool {
        let mut any_event = false;
        let mut resize = false;

        self.input.as_ref().borrow_mut().poll_events();

        // Handle window events
        self.glfw_instance.poll_events();
        let mut input = self.input.as_ref().borrow_mut();
        let mut layouts = self.layouts.as_ref().borrow_mut();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            if input.handle_window_event(&event) {
                any_event = true;
                continue;
            }

            match event {
                glfw::WindowEvent::Size(_, _) => {
                    any_event = true;
                    resize = true;
                },
                glfw::WindowEvent::ContentScale(_, _) => {
                    any_event = true;
                    self.renderer.as_ref().borrow_mut().update_scale(self.get_scale());
                },
                _ => {},
            }
        }
        if resize {
            Self::on_resized_wrapper(
                self.get_size(),
                self.renderer.as_ref().borrow_mut().deref_mut(),
                self.layouts.as_ref().borrow_mut().deref_mut()
            );
        }

        // Handle input events
        if input.event_new_tab {
            input.event_new_tab = false;
            layouts.push(Layout::new(self.get_width(), self.get_height()));
            self.active_layout_idx = self.layouts.len() - 1;
        }
        if input.event_prev_tab {
            input.event_prev_tab = false;
            self.active_layout_idx = (self.active_layout_idx + 1).rem_euclid(layouts.len());
        }
        if input.event_next_tab {
            input.event_next_tab = false;
            self.active_layout_idx = (self.active_layout_idx - 1).rem_euclid(layouts.len());
        }

        any_event
    }

    fn render_wrapper(
        clear_color: &[f32; 3],
        renderer: &mut Renderer,
        layout: &mut Layout,
        window: &mut glfw::PWindow,
        input: &Input
    ) {
        Self::begin_frame(clear_color);
        layout.render(Some(*clear_color), renderer, input);
        Self::end_frame(window);
    }

    fn on_resized_wrapper(
        new_size: [i32; 2],
        renderer: &mut Renderer,
        layouts: &mut Vec<Layout>
    ) {
        renderer.update_viewport_size(new_size[0], new_size[1]);
        for layout in layouts {
            layout.on_window_resized(new_size);
        }
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
