use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::ops::DerefMut;
use std::path::PathBuf;

use cel_core::config::get_config_dir;
use cel_renderer::renderer::Renderer;
use serde::{Deserialize, Serialize};
use anyhow::{Result, bail};

use crate::button::Button;
use crate::input::Input;
use crate::layout::Layout;

#[derive(Serialize, Deserialize, Default)]
struct SessionTab {
    cwd: String
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Vec<SessionTab>
}

pub struct TabGroup {
    width_screen: f32,
    height_screen: f32,
    offset_x_screen: f32,
    offset_y_screen: f32,
    session_file_path: PathBuf,

    layouts: Vec<Layout>,
    active_layout_idx: usize,

    tab_underline_px: f32,
    tab_active_underline_px: f32,
    tab_text_px: f32,
    tab_width_px: f32,
    tab_height_px: f32,
    tab_gap_px: f32,
}

impl TabGroup {
    pub fn new(width_screen: f32, height_screen: f32) -> Self {
        let mut sessions_file_path = get_config_dir();
        sessions_file_path.push("session.json");

        Self {
            width_screen,
            height_screen,
            offset_x_screen: 0.0,
            offset_y_screen: 0.0,
            session_file_path: sessions_file_path,
            
            active_layout_idx: 0,
            layouts: vec![Layout::new(1.0, 1.0)],

            tab_underline_px: 2.0,
            tab_active_underline_px: 2.0,
            tab_text_px: 10.0,
            tab_width_px: 200.0,
            tab_height_px: 25.0,
            tab_gap_px: 2.0,
        }
    }

    pub fn load_session(&mut self) -> Result<()> {
        let file = File::open(&self.session_file_path)?;
        let reader = BufReader::new(file);
        let session: Session = serde_json::from_reader(reader)?;

        self.layouts.clear();
        for tab in session.tabs {
            let mut layout = Layout::new(1.0, 1.0);
            layout.set_current_directory(tab.cwd);
            self.layouts.push(layout);
        }

        log::info!("Session loaded from {}", self.session_file_path.to_str().unwrap());

        // TODO: should resize here, but we don't have renderer
        
        Ok(())
    }

    pub fn write_session(&self) -> Result<()> {
        let mut session: Session = Default::default();
        for layout in &self.layouts {
            let cwd = layout.get_current_directory().to_string();
            if cwd.is_empty() {
                // Do not overwrite the session if any of the tabs are currently still
                // initializing (empty dir)
                bail!("Not ready");
            }
            session.tabs.push(SessionTab { cwd });
        }

        let file = File::create(&self.session_file_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &session)?;
        
        Ok(())
    }

    pub fn update(&mut self, renderer: &Renderer, input: &mut Input) -> bool {
        let mut any_event = false;

        // Update all layouts
        let mut input = input;
        for (i, layout) in self.layouts.iter_mut().enumerate() {
            let input = if i == self.active_layout_idx { Some(input.deref_mut()) } else { None };
            any_event |= layout.update(input);
        }

        // Update active layout only
        //let active_layout = &mut self.layouts[self.active_layout_idx];
        //any_event |= active_layout.update(input);

        // Handle input events
        if input.event_new_tab {
            input.event_new_tab = false;
            self.push_layout(renderer);
            any_event = true;
        }
        if input.event_del_tab {
            input.event_del_tab = false;
            self.pop_active_layout(renderer);
            any_event = true;
        }
        if input.event_prev_tab {
            input.event_prev_tab = false;
            self.active_layout_idx = match self.active_layout_idx {
                0 => self.layouts.len() - 1,
                _ => self.active_layout_idx - 1
            };
            any_event = true;
        }
        if input.event_next_tab {
            input.event_next_tab = false;
            self.active_layout_idx = (self.active_layout_idx + 1) % self.layouts.len();
            any_event = true;
        }

        if any_event {
            let _ = self.write_session();
        }

        any_event
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        bg_color: Option<[f32; 4]>,
        renderer: &mut Renderer,
        input: &Input
    ) -> bool {
        let mut should_rerender = false;

        let opacity = bg_color.map(|c| c[3]).unwrap_or(1.0);
        let err_bg_color = Some([0.3, 0.03, 0.03, opacity]);
        let divider_color = Some([0.133, 0.133, 0.25, opacity]);
        let err_divider_color = Some([0.5, 0.08, 0.08, opacity]);

        let active_layout = &mut self.layouts[self.active_layout_idx];
        should_rerender |= active_layout.render(
            bg_color,
            divider_color,
            err_bg_color,
            err_divider_color,
            renderer,
            input
        );

        // Render tabs
        if self.layouts.len() > 1 {
            let width_px = self.width_screen * renderer.get_width() as f32;
            let tab_width_real = match self.tab_width_px * self.layouts.len() as f32 > width_px {
                true => width_px / self.layouts.len() as f32,
                false => self.tab_width_px
            };
            let max_chars = (tab_width_real / self.tab_text_px).ceil() as usize;
            let underline_y_screen = self.offset_y_screen + (self.tab_height_px - self.tab_active_underline_px) / renderer.get_height() as f32;

            // Tab underline
            renderer.draw_quad(
                &[self.offset_x_screen, underline_y_screen],
                &[self.width_screen, self.tab_underline_px / renderer.get_height() as f32],
                &[0.133, 0.133, 0.25, 1.0]
            );

            let mut cur_offset = self.offset_x_screen * renderer.get_width() as f32;
            for i in 0..self.layouts.len() {
                let is_active = i == self.active_layout_idx;
                let button = Button::new_px(
                    [tab_width_real, self.tab_height_px - self.tab_active_underline_px],
                    [cur_offset, self.offset_y_screen * renderer.get_height() as f32]
                );

                let name = match self.layouts[i].get_name() {
                    "" => "Tab".to_string(),
                    name => if name.len() > max_chars {
                        format!("...{}", &name[name.len().saturating_sub(max_chars)..])
                    } else {
                        name.to_string()
                    }
                };

                let text_color = match is_active {
                    true => [1.0, 1.0, 1.0],
                    false => [0.5, 0.5, 0.5],
                };

                button.render(
                    renderer,
                    &text_color,
                    &[0.0, 0.0, 0.0, 0.0],
                    self.tab_text_px,
                    &name
                );

                if is_active {
                    renderer.draw_quad(
                        &[cur_offset / renderer.get_width() as f32, underline_y_screen],
                        &renderer.to_screen_f32([tab_width_real, self.tab_active_underline_px]),
                        &[0.933, 0.388, 0.321, 1.0]
                    );
                }

                if button.is_clicked(input, glfw::MouseButton::Button1) {
                    self.active_layout_idx = i;
                    should_rerender = true;
                }

                cur_offset += tab_width_real + self.tab_gap_px;
            }
        }

        should_rerender
    }

    pub fn resize(&mut self, renderer: &Renderer, new_size_screen: [f32; 2], new_offset_screen: [f32; 2]) {
        let tab_height_screen = self.tab_height_px / renderer.get_height() as f32;
        let mut real_size = new_size_screen;
        let mut real_offset = new_offset_screen;
        if self.layouts.len() > 1 {
            // Display tabs when >1
            real_size[1] -= tab_height_screen;
            real_offset[1] += tab_height_screen;
        }
        for layout in &mut self.layouts {
            layout.resize(real_size, real_offset);
        }
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let mut text_lines = vec![
            format!("Total tabs: {}", self.layouts.len()),
            format!("Active tab: {}", self.active_layout_idx),
            String::from("\n"),
        ];
        
        let active_layout = &self.layouts[self.active_layout_idx];
        text_lines.extend(active_layout.get_debug_lines());

        text_lines
    }

    fn push_layout(&mut self, renderer: &Renderer) {
        self.layouts.push(Layout::new(1.0, 1.0));
        self.active_layout_idx = self.layouts.len() - 1;

        // Force resize to account for tab offset shift
        self.resize(
            renderer,
            [self.width_screen, self.height_screen],
            [self.offset_x_screen, self.offset_y_screen],
        );
    }

    fn pop_active_layout(&mut self, renderer: &Renderer) {
        if self.layouts.len() <= 1 {
            return;
        }

        self.layouts.remove(self.active_layout_idx);
        self.active_layout_idx = self.active_layout_idx.min(self.layouts.len() - 1);

        // Force resize to account for tab offset shift
        self.resize(
            renderer,
            [self.width_screen, self.height_screen],
            [self.offset_x_screen, self.offset_y_screen],
        );
    }
}
