use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::time::Instant;

use cel_renderer::renderer::Renderer;
use glfw::{fail_on_errors, Context, PWindow};
use macos::get_titlebar_decoration_width_px;

use crate::app_state::AppState;
use crate::input::Input;
use crate::tab_group::TabGroup;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use self::macos::*;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use self::windows::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use self::linux::*;

pub trait PWindowExt {
    fn set_draggable(&mut self, draggable: bool);
    fn get_titlebar_height_px(&self) -> f32;
    fn get_titlebar_decoration_width_px(&self) -> f32;
}

pub struct MonitorInfo {
    pub refresh_rate: u32,
    pub position: (i32, i32),
    pub size: (u32, u32),
}

pub struct Window {
    glfw_instance: glfw::Glfw,
    window: Rc<RefCell<glfw::PWindow>>,
    renderer: Rc<RefCell<Renderer>>,
    tab_group: Rc<RefCell<TabGroup>>,
    input: Rc<RefCell<Input>>,
    event_receiver: glfw::GlfwReceiver<(f64, glfw::WindowEvent)>,
    background_color: [f32; 4],

    last_event_time: Instant,
    last_render_time: Instant,
    rerender_requested: bool,

    debug_widget_width_px: f32,
    debug_show_widget: bool,
}

impl PWindowExt for PWindow {
    // Platform calls

    fn set_draggable(&mut self, draggable: bool) {
        set_draggable(self, draggable);
    }

    fn get_titlebar_height_px(&self) -> f32 {
        get_titlebar_height_px(self)
    }

    fn get_titlebar_decoration_width_px(&self) -> f32 {
        get_titlebar_decoration_width_px(self)
    }
}

impl Window {
    pub fn new() -> Self {
        let mut glfw_instance = glfw::init(fail_on_errors!()).unwrap();

        glfw_instance.window_hint(glfw::WindowHint::TransparentFramebuffer(true));
        glfw_instance.window_hint(glfw::WindowHint::DoubleBuffer(true));
        glfw_instance.window_hint(glfw::WindowHint::Resizable(true));
        glfw_instance.window_hint(glfw::WindowHint::ContextVersion(3, 3));
        glfw_instance.window_hint(glfw::WindowHint::OpenGlForwardCompat(true));
        glfw_instance.window_hint(glfw::WindowHint::OpenGlProfile(
            glfw::OpenGlProfileHint::Core
        ));

        let (mut window, event_receiver) = glfw_instance.create_window(
            1200, 800, "cel_", 
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
        window.set_focus_polling(true);
        window.set_pos_polling(true);
        window.set_resizable(true);

        setup_platform_window(&window);
        
        gl::load_with(|s| window.get_proc_address(s) as *const _);

        glfw_instance.set_swap_interval(glfw::SwapInterval::None);
        //glfw_instance.set_swap_interval(glfw::SwapInterval::Sync(1));

        let scale = window.get_content_scale();
        let initial_size_px = window.get_size();
        let renderer = Renderer::new(
            initial_size_px.0,
            initial_size_px.1,
            scale.into(),
            AppState::current().as_ref().borrow().font.clone()
        );

        let titlebar_height_px = window.get_titlebar_height_px();
        let titlebar_dec_width_px = window.get_titlebar_decoration_width_px();
        let mut tab_group = TabGroup::new(
            &renderer,
            1.0, 1.0,
            titlebar_height_px,
            titlebar_dec_width_px
        );
        let _ = tab_group.load_session(&renderer);

        Self {
            glfw_instance,
            window: Rc::new(RefCell::new(window)),
            renderer: Rc::new(RefCell::new(renderer)),
            tab_group: Rc::new(RefCell::new(tab_group)),
            input: Rc::new(RefCell::new(Input::new())),
            event_receiver,
            background_color: [0.05, 0.05, 0.1, 0.97],

            last_event_time: Instant::now(),
            last_render_time: Instant::now(),
            rerender_requested: true,

            debug_widget_width_px: 300.0,
            debug_show_widget: false,
        }
    }

    pub fn update_and_render(&mut self, dt_ms: f64) {
        let renderer_ptr = self.renderer.clone();
        let tab_group_ptr = self.tab_group.clone();
        let window_ptr = self.window.clone();
        let input_ptr = self.input.clone();
        let clear_color = self.background_color;

        self.window.as_ref().borrow_mut().set_refresh_callback(move |w| {
            let renderer = &mut renderer_ptr.as_ref().borrow_mut();
            let input = &mut input_ptr.as_ref().borrow_mut();
            let tab_group = &mut tab_group_ptr.as_ref().borrow_mut();

            // Update
            Self::on_resized_wrapper(
                true,
                w.get_size().into(),
                renderer,
                tab_group
            );

            // !Glitchy!
            //tab_group.update(renderer, input);

            // Render
            Self::render_wrapper(
                true,
                &clear_color,
                renderer,
                tab_group,
                window_ptr.as_ref().borrow_mut().deref_mut(),
                input,
            );
        });

        // Reduce update frequency if there have not been recent inputs
        let event_time_dist = (Instant::now() - self.last_event_time).as_secs_f32();
        let recently_updated = event_time_dist <= 3.0;
        if !recently_updated {
            let sleep_time = match self.get_is_focused() {
                true => std::time::Duration::from_millis(50),
                false => std::time::Duration::from_millis(100),
            };
            std::thread::sleep(sleep_time);
        }

        let mut any_event = false;
        let mut did_render = false;

        any_event |= self.poll_events();

        {
            // Update tab group
            let renderer = &mut self.renderer.as_ref().borrow_mut();
            let tab_group = &mut self.tab_group.as_ref().borrow_mut();
            any_event |= tab_group.update(
                renderer,
                self.input.as_ref().borrow_mut().deref_mut(),
            );

            if any_event {
                self.last_event_time = Instant::now();
            }

            // Render
            // Only rerender when:
            //  - inputs occurred recently
            //  - rerender is requested
            //  - interval passes, so that blinking effects will render
            //  - another render has not happened too recently
            //  - the debug widget is visible
            let render_time_dist = (Instant::now() - self.last_render_time).as_millis();
            let interval_render = render_time_dist > 250 && self.get_is_focused();
            let very_recent_event = event_time_dist <= 0.05;
            if self.rerender_requested || any_event || interval_render || very_recent_event || self.debug_show_widget {
                //log::warn!("{}, {}", render_time_dist, any_event);
                self.rerender_requested = Self::render_wrapper(
                    false,
                    &self.background_color,
                    renderer,
                    tab_group,
                    self.window.as_ref().borrow_mut().deref_mut(),
                    self.input.as_ref().borrow_mut().deref_mut()
                );
                self.last_render_time = Instant::now();
                did_render = true;
            }
        }

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

        let found = self.glfw_instance.with_connected_monitors(|_, monitors| {
            for monitor in monitors {
                if let Some(video_mode) = monitor.get_video_mode() {
                    let (mon_x, mon_y) = monitor.get_pos();
                    let mon_w = video_mode.width as i32;
                    let mon_h = video_mode.height as i32;

                    // Check if window is within the monitor's bounds
                    // Will fail if window is between monitors / offscreen
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
        });
        
        if let Some(found) = found {
            return Some(found);
        }

        // Attempt to load primary monitor info
        self.glfw_instance.with_primary_monitor(|_, monitor| {
            if let Some(monitor) = monitor {
                if let Some(video_mode) = monitor.get_video_mode() {
                    let (mon_x, mon_y) = monitor.get_pos();
                    return Some(MonitorInfo {
                        refresh_rate: video_mode.refresh_rate,
                        position: (mon_x, mon_y),
                        size: (video_mode.width, video_mode.height),
                    })
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
        let input = self.input.clone();
        let mut input = input.as_ref().borrow_mut();
        for (_, event) in glfw::flush_messages(&self.event_receiver) {
            if input.handle_window_event(&event) {
                any_event = true;
                continue;
            }

            match event {
                glfw::WindowEvent::Size(_, _) => {
                    any_event = true;
                    resize = true;

                    // Dynamically update the tab group inset when switching in/out of
                    // fullscreen, as it may change
                    let new_dec_width = self.window.as_ref().borrow().get_titlebar_decoration_width_px();
                    self.tab_group.as_ref().borrow_mut().set_tab_inset_px(new_dec_width);

                },
                glfw::WindowEvent::ContentScale(_, _) => {
                    any_event = true;
                    self.renderer.as_ref().borrow_mut().update_scale(self.get_scale());
                },
                _ => {},
            }
        }
        if resize {
            let mut tab_group = self.tab_group.as_ref().borrow_mut();
            Self::on_resized_wrapper(
                false,
                self.get_size(),
                self.renderer.as_ref().borrow_mut().deref_mut(),
                &mut tab_group
            );
        }

        // Handle window keystrokes
        if input.get_key_just_pressed(glfw::Key::F5) {
            any_event |= true;
            self.debug_show_widget = !self.debug_show_widget;
        }

        any_event
    }

    fn render_debug_widget(&self, renderer: &mut Renderer, dt_ms: f64) {
        let input = self.input.as_ref().borrow();
        let tab_group = self.tab_group.as_ref().borrow();

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
            String::from("\n"),
        ];

        text_lines.extend(tab_group.get_debug_lines());

        let bg_color = [0.5, 0.1, 0.1, 0.75];
        let size_x = self.debug_widget_width_px / self.get_width() as f32;

        renderer.draw_text(
            10.0,
            &[1.0 - size_x, 0.0],
            &[size_x, 0.75],
            &[1.0, 1.0, 1.0],
            &bg_color,
            false,
            8.0,
            &text_lines.join("\n")
        );
    }

    fn render_wrapper(
        end_frame: bool,
        clear_color: &[f32; 4],
        renderer: &mut Renderer,
        tab_group: &mut TabGroup,
        window: &mut glfw::PWindow,
        input: &mut Input
    ) -> bool {
        Self::begin_frame(clear_color);

        let should_rerender = tab_group.render(Some(*clear_color), renderer, input, window);

        if end_frame {
            Self::end_frame(window);
        }

        should_rerender
    }

    fn on_resized_wrapper(
        soft: bool,
        new_size: [i32; 2],
        renderer: &mut Renderer,
        tab_group: &mut TabGroup
    ) {
        renderer.update_viewport_size(new_size[0], new_size[1]);
        tab_group.resize(
            renderer,
            soft,
            renderer.to_screen_i32(new_size),
            renderer.to_screen_i32([0, 0])
        );
    }

    fn begin_frame(clear_color: &[f32; 4]) {
        unsafe {
            gl::ClearColor(
                clear_color[0],
                clear_color[1],
                clear_color[2],
                clear_color[3]
            );
            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);
        }
    }

    fn end_frame(window: &mut glfw::PWindow) {
        window.swap_buffers();
    }
}
