use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::ops::DerefMut;
use std::path::PathBuf;
use std::process::exit;

use cel_core::config::get_config_dir;
use cel_renderer::renderer::Renderer;
use serde::{Deserialize, Serialize};
use anyhow::{Result, bail};

use crate::button::Button;
use crate::input::{Input, InputEvent};
use crate::layout::Layout;

#[derive(Serialize, Deserialize, Default)]
struct SessionTab {
    cwd: Option<String>,
    char_size_px: Option<f32>,
}

#[derive(Serialize, Deserialize, Default)]
struct Session {
    tabs: Option<Vec<SessionTab>>,
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
    default_char_size_px: f32,
}

impl TabGroup {
    pub fn new(
        renderer: &Renderer,
        width_screen: f32,
        height_screen: f32
    ) -> Self {
        let default_char_size_px = 14.0;
        let mut sessions_file_path = get_config_dir();
        sessions_file_path.push("session.json");

        let default_layout = Layout::new(
            renderer,
            width_screen,
            height_screen,
            default_char_size_px,
            default_char_size_px,
            None
        );

        Self {
            width_screen,
            height_screen,
            offset_x_screen: 0.0,
            offset_y_screen: 0.0,
            session_file_path: sessions_file_path,
            
            active_layout_idx: 0,
            layouts: vec![ default_layout ],

            tab_underline_px: 2.0,
            tab_active_underline_px: 2.0,
            tab_text_px: 10.0,
            tab_width_px: 200.0,
            tab_height_px: 25.0,
            tab_gap_px: 2.0,
            default_char_size_px,
        }
    }

    pub fn load_session(&mut self, renderer: &Renderer) -> Result<()> {
        let file = File::open(&self.session_file_path)?;
        let reader = BufReader::new(file);
        let session: Session = serde_json::from_reader(reader)?;

        if let Some(tabs) = session.tabs {
            self.layouts.clear();
            for tab in tabs {
                self.layouts.push(self.load_layout_from_session_tab(renderer, &tab));
            }
        }

        log::info!("Session loaded from {}", self.session_file_path.to_str().unwrap());

        // Force resize to account for tab offset shift
        self.resize(
            renderer,
            false,
            [self.width_screen, self.height_screen],
            [self.offset_x_screen, self.offset_y_screen],
        );
        
        Ok(())
    }

    pub fn write_session(&self) -> Result<()> {
        let mut session: Session = Default::default();

        let mut tabs = vec![];
        for i in 0..self.layouts.len() {
            let tab = self.serialize_layout_to_session_tab(i);
            if tab.cwd.is_none() || tab.cwd.as_ref().unwrap().is_empty() {
                // Do not overwrite the session if any of the tabs are currently still
                // initializing (empty dir)
                bail!("Not ready");
            }
            tabs.push(tab);
        }
        session.tabs = Some(tabs);

        let file = File::create(&self.session_file_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer(writer, &session)?;
        
        Ok(())
    }

    pub fn update(&mut self, renderer: &Renderer, input: &mut Input) -> bool {
        let mut any_event = false;

        // Update all layouts
        let mut input = input;
        let mut layout_idx = 0;
        loop {
            if layout_idx >= self.layouts.len() {
                break;
            }

            let input = if layout_idx == self.active_layout_idx { Some(input.deref_mut()) } else { None };
            let (layout_event, layout_terminated) = self.layouts[layout_idx].update(renderer, input);
            any_event |= layout_event;

            if layout_terminated {
                self.pop_layout(renderer, layout_idx);
            } else {
                layout_idx += 1;
            }
        }

        // Update active layout only
        //let active_layout = &mut self.layouts[self.active_layout_idx];
        //any_event |= active_layout.update(input);

        // Handle input events
        any_event |= input.consume_event(InputEvent::TabNew, || {
            // Push layout copying settings from active layout
            self.push_layout(
                renderer,
                Some(self.serialize_layout_to_session_tab(self.active_layout_idx))
            );
        });
        any_event |= input.consume_event(InputEvent::TabDelete, || {
            self.pop_active_layout(renderer);
        });
        any_event |= input.consume_event(InputEvent::TabPrev, || {
            self.active_layout_idx = self.active_layout_idx.wrapping_sub(1).min(self.layouts.len() - 1);
        });
        any_event |= input.consume_event(InputEvent::TabNext, || {
            self.active_layout_idx = (self.active_layout_idx + 1) % self.layouts.len();
        });
        any_event |= input.consume_event(InputEvent::TabMoveLeft, || {
            let new_idx = self.active_layout_idx.wrapping_sub(1).min(self.layouts.len() - 1);
            self.layouts.swap(new_idx, self.active_layout_idx);
            self.active_layout_idx = new_idx;
        });
        any_event |= input.consume_event(InputEvent::TabMoveRight, || {
            let new_idx = (self.active_layout_idx + 1) % self.layouts.len();
            self.layouts.swap(new_idx, self.active_layout_idx);
            self.active_layout_idx = new_idx;
        });

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
        input: &mut Input
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

    pub fn resize(
        &mut self,
        renderer: &Renderer,
        soft: bool,
        new_size_screen: [f32; 2],
        new_offset_screen: [f32; 2]
    ) {
        let tab_height_screen = self.tab_height_px / renderer.get_height() as f32;
        let mut real_size = new_size_screen;
        let mut real_offset = new_offset_screen;
        if self.layouts.len() > 1 {
            // Display tabs when >1
            real_size[1] -= tab_height_screen;
            real_offset[1] += tab_height_screen;
        }
        for layout in &mut self.layouts {
            layout.resize(renderer, soft, real_size, real_offset);
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

    fn load_layout_from_session_tab(&self, renderer: &Renderer, tab: &SessionTab) -> Layout {
        let char_size_px = match tab.char_size_px {
            Some(size) => size,
            None => self.default_char_size_px
        };

        Layout::new(
            renderer,
            self.width_screen,
            self.height_screen,
            char_size_px,
            self.default_char_size_px,
            tab.cwd.as_ref().map(|s| s.as_str())
        )
    }

    fn serialize_layout_to_session_tab(&self, layout_idx: usize) -> SessionTab {
        let layout = &self.layouts[layout_idx];
        let cwd = layout.get_current_directory().to_string();
        SessionTab {
            cwd: Some(cwd),
            char_size_px: Some(layout.get_char_size_px()),
        }
    }

    fn push_layout(&mut self, renderer: &Renderer, tab_data: Option<SessionTab>) {
        if let Some(tab_data) = tab_data {
            self.layouts.push(self.load_layout_from_session_tab(renderer, &tab_data));
        } else {
            self.layouts.push(Layout::new(
                renderer,
                self.width_screen,
                self.height_screen,
                self.default_char_size_px,
                self.default_char_size_px,
                None
            ));
        }

        self.active_layout_idx = self.layouts.len() - 1;

        // Force resize to account for tab offset shift
        self.resize(
            renderer,
            false,
            [self.width_screen, self.height_screen],
            [self.offset_x_screen, self.offset_y_screen],
        );
    }

    fn pop_layout(&mut self, renderer: &Renderer, idx: usize) {
        if self.layouts.len() <= 1 {
            // TODO: more graceful exit
            log::info!("No layouts left, exiting");
            exit(0);
        }

        self.layouts.remove(idx);
        self.active_layout_idx = self.active_layout_idx.min(self.layouts.len() - 1);

        // Force resize to account for tab offset shift
        self.resize(
            renderer,
            false,
            [self.width_screen, self.height_screen],
            [self.offset_x_screen, self.offset_y_screen],
        );
    }

    fn pop_active_layout(&mut self, renderer: &Renderer) {
        self.pop_layout(renderer, self.active_layout_idx);
    }
}
