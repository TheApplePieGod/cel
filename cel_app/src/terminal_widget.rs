use cel_core::ansi::{self, AnsiHandler, BufferState, CellContent, MouseTrackingMode};
use cel_renderer::renderer::{Coord, RenderConstants, RenderStats, Renderer};
use cli_clipboard::{ClipboardContext, ClipboardProvider};

use crate::imui::Button;
use crate::{input::{Input, InputEvent}, layout::LayoutPosition};

const MOUSE_BUTTON_MAPPING: [(ansi::MouseButton, glfw::MouseButton); 3] = [
    (ansi::MouseButton::Mouse1, glfw::MouseButton::Button1),
    (ansi::MouseButton::Mouse2, glfw::MouseButton::Button2),
    (ansi::MouseButton::Mouse3, glfw::MouseButton::Button3)
];

pub struct TerminalWidget {
    ansi_handler: AnsiHandler,
    
    last_render_stats: RenderStats,
    last_mouse_pos_cell: [usize; 2],
    last_mouse_pos_widget: [f32; 2],

    // Exclusive
    current_selection_range: [usize; 2],
    is_dragging: bool,

    primary: bool,
    closed: bool,
    expanded: bool,

    padding_px: [f32; 2],

    debug_line_number: bool,
    debug_char_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new(max_rows: u32, max_cols: u32, cwd: Option<&str>) -> Self {
        let max_scrollback = 10000;

        Self {
            ansi_handler: AnsiHandler::new(max_rows, max_cols, max_scrollback, cwd),

            last_render_stats: Default::default(),
            last_mouse_pos_cell: [0, 0],
            last_mouse_pos_widget: [0.0, 0.0],

            current_selection_range: [0, 0],
            is_dragging: false,

            primary: true,
            closed: false,
            expanded: true,

            padding_px: [12.0, 12.0],

            debug_line_number: false,
            debug_char_number: false,
            debug_show_cursor: false
        }
    }

    pub fn consume_output_stream(&mut self) -> Vec<u8> {
        self.ansi_handler.consume_output_stream()
    }

    pub fn push_chars(&mut self, chars: &[u8], stop_early: bool) -> Option<(u32, bool)> {
        self.ansi_handler.handle_sequence_bytes(chars, stop_early)
    }

    pub fn close(&mut self) {
        // Cannot close a primary widget
        if self.primary {
            return;
        }

        self.closed = true;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.ansi_handler.reset();
    }

    pub fn resize(&mut self, max_rows: u32, max_cols: u32, should_clear: bool) {
        self.ansi_handler.resize(max_cols, max_rows, should_clear);
    }

    pub fn reset_render_state(&mut self) {
        self.last_render_stats = Default::default();
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        input: &mut Input,
        widget_pos: &LayoutPosition,
        layout_pos: &LayoutPosition,
        char_size_px: f32,
        min_lines: u32,
        bg_color: Option<[f32; 4]>,
        divider_color: Option<[f32; 4]>,
    ) -> bool {
        let rc = self.get_render_constants(renderer, widget_pos.max_size[0], widget_pos.max_size[1], char_size_px);
        let line_height_screen = rc.char_size_y_screen;

        // Always resize when rendered to ensure reflow has properly occurred
        self.resize(rc.max_rows, rc.max_cols, false);

        let mut real_position = *widget_pos;
        if self.ansi_handler.is_alt_screen_buf_active() {
            // Align the widget such that the first line is at the top of the screen, rather
            // than the bottom always being at the bottom if the lines do not fully fill up
            // the screen space
            let excess_space = widget_pos.max_size[1] - (line_height_screen * self.ansi_handler.get_height() as f32);
            real_position.offset[1] -= excess_space;
        }

        let bg_color = bg_color.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let divider_color = divider_color.unwrap_or([0.1, 0.1, 0.1, 1.0]);
        let bg_height = self.get_height_screen(renderer, &real_position, char_size_px, min_lines);
        self.render_background(renderer, &real_position, bg_height, &bg_color);
        self.render_divider(renderer, &real_position, bg_height, &divider_color);

        self.render_terminal(renderer, &real_position, min_lines, char_size_px, &bg_color);

        self.update_input(renderer, input, &real_position, &rc, bg_height);

        if self.should_handle_text_selection() {
            self.render_selected_text(renderer, &real_position, &rc, bg_height);
        }

        self.render_overlay(input, renderer, &real_position, &layout_pos, bg_height, char_size_px)
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let state = self.ansi_handler.get_terminal_state();
        let cur_elem = self.ansi_handler.get_element(self.last_mouse_pos_cell);
        
        let mut text_lines = vec![
            format!("Total lines: {}", state.grid.screen_buffer.len()),
            format!("Actual height: {}", self.get_num_physical_lines()),
            format!("Max size (cells): {}x{}", self.ansi_handler.get_width(), self.ansi_handler.get_height()),
            format!("Mouse mode: {:?}", state.mouse_mode),
            format!("Mouse tracking mode: {:?}", state.mouse_tracking_mode),
            format!("Hovered cell:"),
            format!("  Screen Pos: ({}, {})", self.last_mouse_pos_cell[0], self.last_mouse_pos_cell[1]),
        ];

        text_lines.extend(
            match cur_elem {
                // TODO: show style
                Some(elem) => vec![match elem.elem {
                    CellContent::Char(c, l) => format!("  Char {:?} (W{})", c, l),
                    CellContent::Grapheme(s, l) => format!("  Grapheme {:?} (W{})", s, l),
                    CellContent::Continuation(i) => format!("  Continuation (-{})", i),
                    CellContent::Empty => format!("  Empty")
                }],
                None => vec![format!("  Uninitialized")]
            }
        );

        text_lines
    }

    pub fn copy_text(&self) {
        match ClipboardContext::new() {
            Ok(mut ctx) => {
                let text = self.ansi_handler.get_text();
                let _ = ctx.set_contents(text);
            }
            Err(err) => {
                log::error!("Failed to initialize clipboard context: {}", err);
            }
        };
    }

    pub fn get_padding(&self, renderer: &Renderer) -> [f32; 2] {
        // No padding is used when the alt screen buffer is active
        match self.ansi_handler.is_alt_screen_buf_active() {
            true => [0.0, 0.0],
            false => [
                self.padding_px[0] / renderer.get_width() as f32,
                self.padding_px[1] / renderer.get_height() as f32
            ]
        }
    }

    pub fn get_line_offset(&self) -> u32 {
        // Ensure the most recent screen is visible (home cursor) when within the 
        // ASB
        match self.ansi_handler.is_alt_screen_buf_active() {
            true => self.ansi_handler.get_terminal_state().grid.get_top_index() as u32,
            false => 0
        }
    }

    pub fn get_render_constants(
        &self,
        renderer: &Renderer,
        screen_width: f32,
        screen_height: f32,
        char_size_px: f32,
    ) -> RenderConstants {
        let padding = self.get_padding(renderer);
        renderer.compute_render_constants(
            screen_width - padding[0] * 2.0,
            screen_height - padding[1] * 2.0,
            char_size_px
        )
    }

    pub fn get_height_screen(
        &self,
        renderer: &Renderer,
        position: &LayoutPosition,
        char_size_px: f32,
        min_lines: u32
    ) -> f32 {
        let rc = self.get_render_constants(renderer, position.max_size[0], position.max_size[1], char_size_px);
        let padding = self.get_padding(renderer);
        let line_size_screen = rc.char_size_y_screen;
        let virtual_lines = self.get_num_virtual_lines(renderer, position.offset[1], line_size_screen, min_lines);
        self.get_num_physical_lines().max(virtual_lines) as f32 * line_size_screen + padding[1] * 2.0
    }

    pub fn get_num_physical_lines(&self) -> usize {
        self.ansi_handler.get_terminal_state().get_num_lines(true)
    }

    pub fn set_primary(&mut self, primary: bool) {
        self.primary = primary;
        if !primary {
            // Make sure to reset this, so that empty prompt blocks do not get cleared
            self.ansi_handler.get_terminal_state_mut().clear_on_resize = false;
        }
    }

    pub fn is_command_running(&self) -> bool {
        // Good heuristic for now (always false after preexec) but maybe will
        // need to change when we introduce support for other shells
        !self.ansi_handler.get_terminal_state().clear_on_resize
    }

    pub fn is_empty(&self) -> bool { self.ansi_handler.is_empty() }
    pub fn is_fullscreen(&self) -> bool { self.ansi_handler.get_terminal_state().alt_screen_buffer_state == BufferState::Active }
    pub fn is_bracketed_paste_enabled(&self) -> bool { self.ansi_handler.get_terminal_state().bracketed_paste_enabled }
    pub fn get_current_dir(&self) -> &str { self.ansi_handler.get_current_dir() }
    pub fn get_exit_code(&self) -> Option<u32> { self.ansi_handler.get_exit_code() }
    pub fn get_last_render_stats(&self) -> &RenderStats { &self.last_render_stats }
    pub fn get_closed(&self) -> bool { self.closed }
    pub fn get_expanded(&self) -> bool { self.expanded }
    pub fn set_expanded(&mut self, expanded: bool) { self.expanded = expanded }
    pub fn get_primary(&self) -> bool { self.primary }
    pub fn toggle_debug_line_numbers(&mut self) { self.debug_line_number = !self.debug_line_number }
    pub fn toggle_debug_char_numbers(&mut self) { self.debug_char_number = !self.debug_char_number }
    pub fn toggle_debug_show_cursor(&mut self) { self.debug_show_cursor = !self.debug_show_cursor }

    fn get_num_virtual_lines(
        &self,
        renderer: &Renderer,
        screen_offset_y: f32,
        line_size_screen: f32,
        min_lines: u32
    ) -> usize {
        let padding = self.get_padding(renderer);
        let line_offset = self.get_line_offset();
        let (num_visible, _, _) = renderer.compute_visible_lines(
            self.ansi_handler.get_terminal_state(),
            line_size_screen,
            line_offset,
            min_lines,
            screen_offset_y - padding[1]
        );
        num_visible
    }

    fn update_input(
        &mut self,
        renderer: &Renderer,
        input: &mut Input,
        position: &LayoutPosition,
        rc: &RenderConstants,
        bg_height: f32,
    ) {
        // Compute the target cell based on the mouse position and widget position

        let padding = self.get_padding(renderer);

        let virtual_lines = ((bg_height - padding[1] * 2.0) / rc.char_size_y_screen) as u32;
        let max_rows = self.ansi_handler.get_height();
        let height = rc.char_size_y_screen * virtual_lines as f32;
        let width = rc.char_size_x_screen * rc.max_cols as f32;
        let mouse_pos_px = input.get_mouse_pos();
        let mouse_pos_screen = [
            mouse_pos_px[0] / renderer.get_width() as f32,
            mouse_pos_px[1] / renderer.get_height() as f32,
        ];
        let mouse_pos_widget = [
            (mouse_pos_screen[0] - (position.offset[0] + padding[0])) / width,
            (mouse_pos_screen[1] - (position.offset[1] - bg_height + padding[1])) / height,
        ];

        let widget_row = virtual_lines as f32 * mouse_pos_widget[1];
        let widget_col = rc.max_cols as f32 * mouse_pos_widget[0];
        let screen_row =
            widget_row +
            max_rows.min(virtual_lines) as f32 -
            virtual_lines as f32;
        let screen_col = widget_col;

        let mut key_flags: ansi::KeyboardModifierFlags = ansi::KeyboardModifierFlags::default();
        if input.is_shift_down() {
            key_flags.insert(ansi::KeyboardModifierFlags::Shift);
        }
        if input.is_super_down() {
            key_flags.insert(ansi::KeyboardModifierFlags::Meta);
        }
        if input.is_ctrl_down() {
            key_flags.insert(ansi::KeyboardModifierFlags::Control);
        }
        if input.is_alt_down() {
            key_flags.insert(ansi::KeyboardModifierFlags::Alt);
        }

        // Perform bounds checking to see if the mouse is currently within the "active"
        // part of the widget, i.e below [0, 0] in terminal space
        if screen_col >= 0.0 && screen_col < rc.max_cols as f32 && screen_row >= 0.0 && screen_row < virtual_lines as f32 {
            let cursor = [screen_col as usize, screen_row as usize];

            // Only send mouse inputs to active widget
            if self.primary {
                for entry in MOUSE_BUTTON_MAPPING {
                    self.ansi_handler.handle_mouse_button(
                        entry.0,
                        input.get_mouse_down(entry.1),
                        key_flags,
                        &cursor
                    );
                }

                // Empirical
                #[cfg(target_os = "macos")]
                let scroll_scale = 4.0;
                #[cfg(target_os = "linux")]
                let scroll_scale = 16.0;
                #[cfg(target_os = "windows")]
                let scroll_scale = 16.0;

                let scroll_scale_x = scroll_scale / rc.char_size_x_px;
                let scroll_scale_y = scroll_scale / rc.char_size_y_px;
                let scroll_delta = input.get_scroll_delta();
                self.ansi_handler.handle_scroll(
                    scroll_delta[0] * scroll_scale_x,
                    scroll_delta[1] * scroll_scale_y,
                    key_flags,
                    &cursor
                );
            }

            self.last_mouse_pos_cell = cursor;
        }

        if self.should_handle_text_selection() {
            // Handle text dragging when with the widget bounds
            if widget_row >= 0.0 && widget_row < virtual_lines as f32 && widget_col >= 0.0 && widget_col < rc.max_cols as f32 {
                let cur_widget_pos = (widget_row as u32 * rc.max_cols + widget_col.round() as u32) as usize; 
                if input.get_mouse_just_pressed(glfw::MouseButton::Button1) {
                    self.is_dragging = true;
                    self.current_selection_range = [cur_widget_pos, cur_widget_pos];
                }
                if self.is_dragging {
                    self.current_selection_range[1] = cur_widget_pos;
                }
            } else if input.get_mouse_just_pressed(glfw::MouseButton::Button1) {
                // Otherwise clear selection when clicking
                self.current_selection_range = [0, 0];
            }

            // Always reset dragging state on mouse release
            if !input.get_mouse_down(glfw::MouseButton::Button1) {
                self.is_dragging = false;
            }

            // Try to consume copy, if we have a selected region
            input.maybe_consume_event(InputEvent::Copy, || {
                if self.current_selection_range[0] == self.current_selection_range[1] {
                    return false;
                }

                match ClipboardContext::new() {
                    Ok(mut ctx) => {
                        let grid = &self.ansi_handler.get_terminal_state().grid;

                        let mut lines = vec![];
                        self.iter_selected_region(rc.max_cols, |y, start_x, end_x| {
                            if y >= grid.screen_buffer.len() {
                                return;
                            }

                            let line = &grid.screen_buffer[y];
                            let mut line_text = String::new();
                            for x in start_x..end_x {
                                if x >= line.len() {
                                    break;
                                }

                                grid.append_cell_content(&line[x], &mut line_text);
                            }

                            lines.push(line_text);
                        });

                        let text = lines.join("\n");
                        if !text.is_empty() {
                            let _ = ctx.set_contents(text);
                        }

                        // Reset selection
                        //self.current_selection_range = [0, 0];

                        true
                    }
                    Err(err) => {
                        log::error!("Failed to initialize clipboard context: {}", err);
                        false
                    }
                }
            });
        } else {
            // Ensure state is reset
            self.is_dragging = false;
            self.current_selection_range = [0, 0];
        }

        // Only send special key inputs to active widget
        if self.primary && key_flags.is_empty() {
            // Handle special arrow key encoding

            if input.get_key_triggered(glfw::Key::Up) {
                self.ansi_handler.handle_up_arrow();
            }
            if input.get_key_triggered(glfw::Key::Down) {
                self.ansi_handler.handle_down_arrow();
            }
            if input.get_key_triggered(glfw::Key::Left) {
                self.ansi_handler.handle_left_arrow();
            }
            if input.get_key_triggered(glfw::Key::Right) {
                self.ansi_handler.handle_right_arrow();
            }
        }

        // Recompute position to incorporate padding
        let mouse_pos_widget = [
            (mouse_pos_screen[0] - position.offset[0]) / (width + padding[0] * 2.0),
            (mouse_pos_screen[1] - (position.offset[1] - bg_height)) / (height + padding[1] * 2.0),
        ];
        self.last_mouse_pos_widget = mouse_pos_widget;
    }

    fn render_background(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        height: f32,
        bg_color: &[f32; 4]
    ) {
        renderer.draw_ui_quad(
            &Coord::Screen([position.offset[0], position.offset[1] - height]),
            &Coord::Screen([1.0, height]),
            bg_color,
            0.0
        );
    }

    fn render_divider(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        height: f32,
        color: &[f32; 4]
    ) {
        let size_px = 2.0;
        let size = size_px / renderer.get_height() as f32;
        renderer.draw_ui_quad(
            &Coord::Screen([position.offset[0], position.offset[1] - height]),
            &Coord::Screen([1.0, size]),
            color,
            0.0
        );
    }

    fn render_selected_text(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        rc: &RenderConstants,
        height: f32
    ) {
        renderer.enable_blending();

        let padding = self.get_padding(renderer);
        self.iter_selected_region(rc.max_cols, |y, start_x, end_x| {
            renderer.draw_ui_quad(
                &Coord::Screen([
                    position.offset[0] + padding[0] + start_x as f32 * rc.char_size_x_screen,
                    position.offset[1] - height + padding[1] + y as f32 * rc.char_size_y_screen
                ]),
                &Coord::Screen([(end_x - start_x) as f32 * rc.char_size_x_screen, rc.char_size_y_screen]),
                &[0.25, 0.5, 0.5, 0.3],
                0.0
            );
        });

        renderer.disable_blending();
    }

    fn render_terminal(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        min_lines: u32,
        char_size_px: f32,
        bg_color: &[f32; 4]
    ) {

        let padding = self.get_padding(renderer);

        let term_color = [bg_color[0], bg_color[1], bg_color[2]];
        self.ansi_handler.set_terminal_color(&term_color);
        if !self.primary {
            self.ansi_handler.hide_cursor();
        }

        let render_offset = [
            position.offset[0] + padding[0],
            position.offset[1] - padding[1]
        ];
        let render_size = [
            position.max_size[0] - 2.0 * padding[0],
            position.max_size[1] - 2.0 * padding[1],
        ];

        let render_stats = renderer.render_terminal(
            &self.ansi_handler.get_terminal_state(),
            &Coord::Screen(render_size),
            &Coord::Screen(render_offset),
            char_size_px,
            self.get_line_offset(),
            min_lines,
            self.debug_line_number,
            self.debug_char_number,
            self.debug_show_cursor
        );

        self.last_render_stats = render_stats;
    }

    // Returns true if a rerender should occur after this one, i.e. when a button is pressed
    fn render_overlay(
        &mut self,
        input: &Input,
        renderer: &mut Renderer,
        widget_pos: &LayoutPosition,
        layout_pos: &LayoutPosition,
        bg_height: f32,
        char_size_px: f32
    ) -> bool {
        if self.primary || !self.is_mouse_in_widget() {
            return false;
        }

        let mut should_rerender = false;

        let icons = ["📋", "❌"];
        let icon_size_px = char_size_px * 1.25;
        let icon_gap_px = icon_size_px * 0.25;
        let icon_padding_px = char_size_px;
        for (i, icon) in icons.into_iter().enumerate() {
            let x_pos = icon_size_px * (i as f32 + 1.0) + icon_gap_px * i as f32 + icon_padding_px;
            let y_pos = ((widget_pos.offset[1] - bg_height).max(layout_pos.offset[1])) * renderer.get_height() as f32;
            let button = Button::new()
                .size(Coord::Px([icon_size_px, icon_size_px]))
                .offset(Coord::Px([renderer.get_width() as f32 - x_pos, y_pos + icon_padding_px]))
                .fg_color([1.0, 1.0, 1.0])
                .char_height_px(icon_size_px)
                .text(icon)
                .render(renderer);

            if button.is_clicked(renderer, input, glfw::MouseButton::Button1) {
                should_rerender = true;
                match i {
                    // Copy
                    0 => self.copy_text(),
                    // Close
                    1 => self.close(),
                    _ => {}
                }
            }
        }

        should_rerender
    }

    fn should_handle_text_selection(&self) -> bool {
        // Disable selection when mouse reporting is enabled 
        self.ansi_handler.get_terminal_state().mouse_tracking_mode == MouseTrackingMode::Disabled
    }

    fn is_mouse_in_widget(&self) -> bool {
        self.last_mouse_pos_widget[0] >= 0.0 && self.last_mouse_pos_widget[0] < 1.0 &&
        self.last_mouse_pos_widget[1] >= 0.0 && self.last_mouse_pos_widget[1] < 1.0
    }

    fn iter_selected_region(&self, max_cols: u32, mut fun: impl FnMut(usize, usize, usize)) {
        let max_cols = max_cols as usize;
        let min_idx = self.current_selection_range.iter().min().unwrap();
        let max_idx = self.current_selection_range.iter().max().unwrap();
        let start_y = min_idx / max_cols;
        let end_y = max_idx / max_cols;
        for y in start_y..=end_y {
            let start_x = match y == start_y {
                true => min_idx % max_cols,
                false => 0
            };
            let end_x = match y == end_y {
                true => max_idx % max_cols,
                false => max_cols
            };

            if start_x != end_x {
                fun(y, start_x, end_x);
            }
        }
    }
}
