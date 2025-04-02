use std::borrow::{Borrow, BorrowMut};
use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::time::Instant;

use cel_renderer::renderer::Renderer;
use glfw::{Context, fail_on_errors};

use crate::app_state::AppState;
use crate::layout::Layout;
use crate::input::Input;

pub struct MonitorInfo {
    pub refresh_rate: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
}

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: Rc<RefCell<glfw::PWindow>>,
    renderer: Rc<RefCell<Renderer>>,
    layouts: Rc<RefCell<Vec<Layout>>>,
    input: Rc<RefCell<Input>>,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    background_color: [f32; 3],
    active_layout_idx: usize,

    last_event_time: Instant,
    last_render_time: Instant,
    rerender_requested: bool,

    debug_widget_width_px: f32,
    debug_show_widget: bool,
}

impl Window {
    pub fn new() -> Self {
        let mut glfw_instance = glfw::init(fail_on_errors!()).unwrap();

        //glfw_instance.window_hint(glfw::WindowHint::Decorated(false));
        glfw_instance.window_hint(glfw::WindowHint::DoubleBuffer(true));
        glfw_instance.window_hint(glfw::WindowHint::Resizable(true));
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

        glfw_instance.set_swap_interval(glfw::SwapInterval::None);
        //glfw_instance.set_swap_interval(glfw::SwapInterval::Sync(1));

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
            active_layout_idx: 0,

            last_event_time: Instant::now(),
            last_render_time: Instant::now(),
            rerender_requested: true,

            debug_widget_width_px: 300.0,
            debug_show_widget: false,
        }
    }

    pub fn update_and_render(&mut self, dt_ms: f64) {
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

            let active_layout = &mut layouts[active_layout_idx];

            // !Glitchy!
            //active_layout.update(input_ptr.as_ref().borrow().deref());

            // Render
            Self::render_wrapper(
                true,
                &clear_color,
                renderer_ptr.as_ref().borrow_mut().deref_mut(),
                active_layout,
                window_ptr.as_ref().borrow_mut().deref_mut(),
                input_ptr.as_ref().borrow().deref()
            );
        });

        // Reduce update frequency if there have not been recent inputs
        let event_time_dist = (Instant::now() - self.last_event_time).as_secs_f32();
        let recently_updated = event_time_dist <= 3.0;
        if !recently_updated {
            std::thread::sleep(std::time::Duration::new(0, 50e6 as u32));
        }

        let mut any_event = false;
        let mut did_render = false;

        any_event |= self.poll_events();

        {
            // Update layout
            let active_layout = &mut self.layouts.as_ref().borrow_mut()[self.active_layout_idx];
            any_event |= active_layout.update(self.input.as_ref().borrow().deref());

            if any_event {
                self.last_event_time = Instant::now();
            }

            // Render
            // Only rerender when:
            //  - inputs occurred recently
            //  - rerender is requested
            //  - interval passes, so that blinking effects will render
            //  - another render has not happened too recently
            let render_time_dist = (Instant::now() - self.last_render_time).as_millis();
            let interval_render = render_time_dist > 250 && self.get_is_focused();
            let very_recent_event = event_time_dist <= 0.05;
            if self.rerender_requested || any_event || interval_render || very_recent_event {
                //log::warn!("{}, {}", render_time_dist, any_event);
                self.rerender_requested = Self::render_wrapper(
                    false,
                    &self.background_color,
                    self.renderer.as_ref().borrow_mut().deref_mut(),
                    active_layout,
                    self.window.as_ref().borrow_mut().deref_mut(),
                    self.input.as_ref().borrow().deref()
                );
                self.last_render_time = Instant::now();
                did_render = true;
            }
        }

        // Always rerender debug widget
        if self.debug_show_widget {
            self.render_debug_widget(
                self.renderer.as_ref().borrow_mut().deref_mut(),
                dt_ms
            );
            did_render = true;
        }

        // Need to present
        if did_render {
            Self::end_frame(self.window.as_ref().borrow_mut().deref_mut());
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
    pub fn get_is_focused(&self) -> bool { self.window.as_ref().borrow().is_focused() }
    pub fn get_time_seconds(&self) -> f64 { self.glfw_instance.get_time() }
    pub fn get_monitor_info(&mut self) -> Option<MonitorInfo> { self.get_monitor() }

    fn get_monitor(&mut self) -> Option<MonitorInfo> {
        let window = self.window.as_ref().borrow();
        let (win_x, win_y) = window.get_pos();
        let (win_w, win_h) = window.get_size();

        self.glfw_instance.with_connected_monitors(|_, monitors| {
            for monitor in monitors {
                if let Some(video_mode) = monitor.get_video_mode() {
                    let (mon_x, mon_y) = monitor.get_pos();
                    let mon_w = video_mode.width as i32;
                    let mon_h = video_mode.height as i32;

                    // Check if window is within the monitor's bounds
                    if win_x >= mon_x && win_x + win_w <= mon_x + mon_w &&
                       win_y >= mon_y && win_y + win_h <= mon_y + mon_h {
                        return Some(MonitorInfo {
                            refresh_rate: video_mode.refresh_rate,
                            position: (mon_x, mon_y),
                            size: (video_mode.width, video_mode.height),
                        });
                    }
                }
            }
            None
        })
    }

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
                &mut layouts
            );
        }

        // Handle input events
        if input.event_new_tab {
            input.event_new_tab = false;
            layouts.push(Layout::new(self.get_width(), self.get_height()));
            self.active_layout_idx = layouts.len() - 1;
        }
        if input.event_del_tab {
            input.event_del_tab = false;
            if layouts.len() > 1 {
                layouts.remove(self.active_layout_idx);
                self.active_layout_idx = self.active_layout_idx.min(
                    layouts.len() - 1
                );
            }
        }
        if input.event_prev_tab {
            input.event_prev_tab = false;
            self.active_layout_idx = match self.active_layout_idx {
                0 => layouts.len() - 1,
                _ => self.active_layout_idx - 1
            };
        }
        if input.event_next_tab {
            input.event_next_tab = false;
            self.active_layout_idx = (self.active_layout_idx + 1) % layouts.len();
        }

        // Handle window keystrokes
        if input.get_key_just_pressed(glfw::Key::F5) {
            any_event |= true;
            self.debug_show_widget = !self.debug_show_widget;
        }

        any_event
    }

    fn render_debug_widget(&self, renderer: &mut Renderer, dt_ms: f64) {
        let layouts = self.layouts.as_ref().borrow();
        let input = self.input.as_ref().borrow();

        let mouse_screen = [
            input.get_mouse_pos()[0] / self.get_width() as f32,
            input.get_mouse_pos()[1] / self.get_height() as f32
        ];
        let mut text_lines: Vec<String> = vec![
            String::from("cel_ debug"),
            format!("Frametime (ms): {:.1}", dt_ms),
            format!("Window size: {}x{}", self.get_width(), self.get_height()),
            format!("Mouse pos: ({}, {})", mouse_screen[0], mouse_screen[1]),
            format!("Content scale: {}x{}", self.get_scale()[0], self.get_scale()[1]),
            format!("Num layouts: {}", layouts.len()),
            format!("Active layout: {}", self.active_layout_idx),
            String::from("\n"),
        ];

        
        let active_layout = &layouts[self.active_layout_idx];
        text_lines.extend(active_layout.get_debug_lines());

        let bg_color = [0.5, 0.1, 0.1];
        let size_x = self.debug_widget_width_px / self.get_width() as f32;
        renderer.draw_quad(
            &[1.0 - size_x, 0.0],
            &[size_x, 0.5],
            &bg_color
        );

        let chars_per_line = self.get_width() as f32 / 6.0;
        renderer.draw_text(
            chars_per_line as u32,
            &[1.0 - size_x, 0.0],
            &[0.0, 0.0],
            &[1.0, 1.0, 1.0],
            &bg_color,
            false,
            &text_lines.join("\n")
        );
    }

    fn render_wrapper(
        end_frame: bool,
        clear_color: &[f32; 3],
        renderer: &mut Renderer,
        layout: &mut Layout,
        window: &mut glfw::PWindow,
        input: &Input
    ) -> bool {
        Self::begin_frame(clear_color);
        let should_rerender = layout.render(Some(*clear_color), renderer, input);
        if end_frame {
            Self::end_frame(window);
        }

        should_rerender
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

    fn begin_frame(clear_color: &[f32; 3]) {
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
