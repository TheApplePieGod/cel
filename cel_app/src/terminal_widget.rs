use cel_core::ansi::{self, AnsiHandler, BufferState, CellContent};
use cel_renderer::renderer::{RenderConstants, RenderStats, Renderer};
use cli_clipboard::{ClipboardContext, ClipboardProvider};

use crate::{button::Button, input::Input, layout::LayoutPosition, terminal_context::TerminalContext};

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

    primary: bool,
    closed: bool,
    expanded: bool,

    padding_px: [f32; 2],
    overlay_padding_px: [f32; 2],
    icon_size_px: f32,
    icon_gap_px: f32,

    debug_line_number: bool,
    debug_char_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new(max_rows: u32, max_cols: u32) -> Self {
        Self {
            ansi_handler: AnsiHandler::new(max_rows, max_cols),

            last_render_stats: Default::default(),
            last_mouse_pos_cell: [0, 0],
            last_mouse_pos_widget: [0.0, 0.0],

            primary: true,
            closed: false,
            expanded: true,

            padding_px: [12.0, 12.0],
            overlay_padding_px: [6.0, 6.0],
            icon_size_px: 16.0,
            icon_gap_px: 4.0,

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
        input: &Input,
        position: &LayoutPosition,
        char_size_px: f32,
        min_lines: u32,
        bg_color: Option<[f32; 4]>,
        divider_color: Option<[f32; 4]>,
    ) -> bool {
        let padding = self.get_padding(renderer);
        let max_rows = renderer.get_max_lines(position.max_size[1] - padding[1] * 2.0, char_size_px);
        let max_cols = renderer.get_chars_per_line(position.max_size[0] - padding[0] * 2.0, char_size_px);
        let rc = renderer.compute_render_constants(position.max_size[0], max_cols);
        let line_height_screen = rc.char_size_y_screen;

        // Always resize when rendered to ensure reflow has properly occurred
        self.resize(max_rows, max_cols, false);

        let mut real_position = *position;
        if self.ansi_handler.is_alt_screen_buf_active() {
            // Align the widget such that the first line is at the top of the screen, rather
            // than the bottom always being at the bottom if the lines do not fully fill up
            // the screen space
            let excess_space = position.max_size[1] - (line_height_screen * self.ansi_handler.get_height() as f32);
            real_position.offset[1] -= excess_space;
        }

        let bg_color = bg_color.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let divider_color = divider_color.unwrap_or([0.1, 0.1, 0.1, 1.0]);
        let bg_height = self.get_height_screen(renderer, real_position.offset[1], real_position.max_size[0], char_size_px, min_lines);
        self.render_background(renderer, &real_position, bg_height, &bg_color);
        self.render_divider(renderer, &real_position, bg_height, &divider_color);

        self.render_terminal(renderer, &real_position, min_lines, max_cols, &bg_color);

        self.update_mouse_input(renderer, input, &real_position, &rc, bg_height, max_cols);

        self.render_overlay(input, renderer, &real_position, bg_height)
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let state = self.ansi_handler.get_terminal_state();
        let cur_elem = self.ansi_handler.get_element(self.last_mouse_pos_cell);
        
        let mut text_lines = vec![
            format!("Total lines: {}", state.grid.screen_buffer.len()),
            format!("Actual height: {}", self.get_num_physical_lines()),
            format!("Max size (cells): {}x{}", self.ansi_handler.get_width(), self.ansi_handler.get_height()),
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

    pub fn get_height_screen(
        &self,
        renderer: &Renderer,
        screen_width: f32,
        screen_offset_y: f32,
        char_size_px: f32,
        min_lines: u32
    ) -> f32 {
        let padding = self.get_padding(renderer);
        let max_cols = renderer.get_chars_per_line(screen_width, char_size_px);
        let rc = renderer.compute_render_constants(screen_width, max_cols);
        let line_size_screen = rc.char_size_y_screen;
        let virtual_lines = self.get_num_virtual_lines(renderer, screen_offset_y, line_size_screen, min_lines);
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

    fn update_mouse_input(
        &mut self,
        renderer: &Renderer,
        input: &Input,
        position: &LayoutPosition,
        rc: &RenderConstants,
        bg_height: f32,
        max_cols: u32
    ) {
        // Compute the target cell based on the mouse position and widget position

        let padding = self.get_padding(renderer);

        let virtual_lines = ((bg_height - padding[1] * 2.0) / rc.char_size_y_screen) as u32;
        let max_rows = self.ansi_handler.get_height();
        let height = rc.char_size_y_screen * virtual_lines as f32;
        let width = rc.char_size_x_screen * max_cols as f32;
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
        let screen_row =
            widget_row +
            max_rows.min(virtual_lines) as f32 -
            virtual_lines as f32;
        let screen_col = max_cols as f32 * mouse_pos_widget[0];

        if screen_col >= 0.0 && screen_col < max_cols as f32 && screen_row >= 0.0 && screen_row < virtual_lines as f32 {
            let cursor = [screen_col as usize, screen_row as usize];

            // Only send inputs to active widget
            if self.primary {
                let mut flags: ansi::KeyboardModifierFlags = ansi::KeyboardModifierFlags::default();
                if input.is_shift_down() {
                    flags.insert(ansi::KeyboardModifierFlags::Shift);
                }
                if input.is_super_down() {
                    flags.insert(ansi::KeyboardModifierFlags::Meta);
                }
                if input.is_ctrl_down() {
                    flags.insert(ansi::KeyboardModifierFlags::Control);
                }
                if input.is_alt_down() {
                    flags.insert(ansi::KeyboardModifierFlags::Alt);
                }

                for entry in MOUSE_BUTTON_MAPPING {
                    self.ansi_handler.handle_mouse_button(
                        entry.0,
                        input.get_mouse_down(entry.1),
                        flags,
                        &cursor
                    );
                }

                let scroll_scale_x = 4.0 / rc.char_size_x_px; // Empirical
                let scroll_scale_y = 4.0 / rc.char_size_y_px; // Empirical
                let scroll_delta = input.get_scroll_delta();
                self.ansi_handler.handle_scroll(
                    scroll_delta[0] * scroll_scale_x,
                    scroll_delta[1] * scroll_scale_y,
                    flags,
                    &cursor
                );
            }

            self.last_mouse_pos_cell = cursor;
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
        renderer.draw_quad(
            &[position.offset[0], position.offset[1] - height],
            &[1.0, height],
            bg_color
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
        renderer.draw_quad(
            &[position.offset[0], position.offset[1] - height],
            &[1.0, size],
            color
        );
    }

    fn render_terminal(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        min_lines: u32,
        max_cols: u32,
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
            &render_size,
            &render_offset,
            max_cols,
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
        position: &LayoutPosition,
        bg_height: f32,
    ) -> bool {
        if self.primary || !self.is_mouse_in_widget() {
            return false;
        }

        let mut should_rerender = false;

        let icons = ["📋", "❌"];
        for (i, icon) in icons.into_iter().enumerate() {
            let x_pos = self.icon_size_px * (i as f32 + 1.0) + self.icon_gap_px * i as f32 + self.overlay_padding_px[0];
            let button = Button::new_px(
                [self.icon_size_px, self.icon_size_px],
                [
                    renderer.get_width() as f32 - x_pos,
                    (position.offset[1] - bg_height) * renderer.get_height() as f32 + self.overlay_padding_px[1]
                ]
            );

            button.render(
                renderer,
                &[1.0, 1.0, 1.0],
                &[0.0, 0.0, 0.0, 0.0],
                self.icon_size_px,
                icon
            );

            if button.is_clicked(input, glfw::MouseButton::Button1) {
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

    fn is_mouse_in_widget(&self) -> bool {
        self.last_mouse_pos_widget[0] >= 0.0 && self.last_mouse_pos_widget[0] < 1.0 &&
        self.last_mouse_pos_widget[1] >= 0.0 && self.last_mouse_pos_widget[1] < 1.0
    }
}
