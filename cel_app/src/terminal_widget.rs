use cel_core::ansi::{self, AnsiHandler, BufferState, CellContent};
use cel_renderer::renderer::{RenderStats, Renderer};
use cli_clipboard::{ClipboardContext, ClipboardProvider};

use crate::{button::Button, input::Input, layout::LayoutPosition};

const MOUSE_BUTTON_MAPPING: [(ansi::MouseButton, glfw::MouseButton); 3] = [
    (ansi::MouseButton::Mouse1, glfw::MouseButton::Button1),
    (ansi::MouseButton::Mouse2, glfw::MouseButton::Button2),
    (ansi::MouseButton::Mouse3, glfw::MouseButton::Button3)
];

pub struct TerminalWidget {
    ansi_handler: AnsiHandler,
    chars_per_line: u32,
    lines_per_screen: u32,
    
    last_render_stats: RenderStats,
    last_computed_height_screen: f32,
    last_rendered_lines: u32,
    last_line_height_screen: f32,
    last_char_width_screen: f32,
    last_mouse_pos_cell: [usize; 2],
    last_mouse_pos_widget: [f32; 2],

    just_closed: bool,

    primary: bool,
    closed: bool,
    expanded: bool,
    wrap: bool,

    padding_px: [f32; 2],
    overlay_padding_px: [f32; 2],
    icon_size_px: f32,
    icon_gap_px: f32,

    debug_line_number: bool,
    debug_char_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new() -> Self {
        let chars_per_line = 180; // Sane default size
        let lines_per_screen = 40;

        Self {
            ansi_handler: AnsiHandler::new(chars_per_line, lines_per_screen),
            chars_per_line,
            lines_per_screen,

            last_render_stats: Default::default(),
            last_computed_height_screen: 0.0,
            last_rendered_lines: 0,
            last_line_height_screen: 1.0, // To prevent the 'excess space' from blowing up on initial render
            last_char_width_screen: 0.0,
            last_mouse_pos_cell: [0, 0],
            last_mouse_pos_widget: [0.0, 0.0],

            just_closed: false,

            primary: true,
            closed: false,
            expanded: true,
            wrap: true,

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
        self.just_closed = true;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.ansi_handler.reset();
    }

    pub fn reset_render_state(&mut self) {
        self.last_render_stats = Default::default();
        self.just_closed = false
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        input: &Input,
        position: &LayoutPosition,
        char_size_px: f32,
        default_height: f32,
        bg_color: Option<[f32; 4]>,
        divider_color: Option<[f32; 4]>,
    ) -> bool {
        // Align the widget such that the first line is at the top of the screen, rather
        // than the bottom always being at the bottom if the lines do not fully fill up
        // the screen space
        let excess_space = position.max_size[1] - (self.last_line_height_screen * self.lines_per_screen as f32);
        let mut real_position = *position;
        real_position.offset[1] -= excess_space;

        self.update_mouse_input(renderer, input, &real_position, char_size_px);

        let bg_color = bg_color.unwrap_or([0.0, 0.0, 0.0, 1.0]);
        let divider_color = divider_color.unwrap_or([0.1, 0.1, 0.1, 1.0]);
        self.render_background(renderer, &real_position, default_height, &bg_color);
        self.render_divider(renderer, &real_position, &divider_color);
        self.render_terminal(renderer, &real_position, char_size_px, default_height, &bg_color);

        self.render_overlay(input, renderer, &real_position)
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let state = self.ansi_handler.get_terminal_state();
        let active_size = self.get_terminal_size();
        let cur_elem = self.ansi_handler.get_element(self.last_mouse_pos_cell);
        
        let mut text_lines = vec![
            format!("Active size: {}x{}", active_size[0], active_size[1]),
            format!("Total lines: {}", state.screen_buffer.len()),
            format!("Rendered lines: {}", self.last_rendered_lines),
            format!("Height (screen): {}", self.last_computed_height_screen),
            format!("Line height: {}", self.last_line_height_screen),
            format!("Char width: {}", self.last_char_width_screen),
            format!("Chars per line: {}", self.chars_per_line),
            format!("Hovered cell:"),
            format!("  Pos: ({}, {})", self.last_mouse_pos_cell[0], self.last_mouse_pos_cell[1]),
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

    pub fn is_empty(&self) -> bool { self.ansi_handler.is_empty() }
    pub fn is_fullscreen(&self) -> bool { self.ansi_handler.get_terminal_state().alt_screen_buffer_state == BufferState::Active }
    pub fn is_bracketed_paste_enabled(&self) -> bool { self.ansi_handler.get_terminal_state().bracketed_paste_enabled }
    pub fn get_current_dir(&self) -> &str { self.ansi_handler.get_current_dir() }
    pub fn get_exit_code(&self) -> Option<u32> { self.ansi_handler.get_exit_code() }
    pub fn get_last_render_stats(&self) -> &RenderStats { &self.last_render_stats }
    pub fn get_last_computed_height_screen(&self) -> f32 { self.last_computed_height_screen }
    pub fn get_closed(&self) -> bool { self.closed }
    pub fn get_just_closed(&self) -> bool { self.just_closed }
    pub fn get_expanded(&self) -> bool { self.expanded }
    pub fn set_expanded(&mut self, expanded: bool) { self.expanded = expanded }
    pub fn get_primary(&self) -> bool { self.primary }
    pub fn set_primary(&mut self, primary: bool) { self.primary = primary }
    pub fn get_terminal_size(&self) -> [u32; 2] { [self.chars_per_line, self.lines_per_screen] }
    pub fn toggle_debug_line_numbers(&mut self) { self.debug_line_number = !self.debug_line_number }
    pub fn toggle_debug_char_numbers(&mut self) { self.debug_char_number = !self.debug_char_number }
    pub fn toggle_debug_show_cursor(&mut self) { self.debug_show_cursor = !self.debug_show_cursor }

    fn update_mouse_input(
        &mut self,
        renderer: &Renderer,
        input: &Input,
        position: &LayoutPosition,
        char_size_px: f32
    ) {
        // Compute the target cell based on the mouse position and widget position

        let padding = match self.ansi_handler.is_alt_screen_buf_active() {
            true => [0.0, 0.0],
            false => [
                self.padding_px[0] / renderer.get_width() as f32,
                self.padding_px[1] / renderer.get_height() as f32
            ]
        };
        let last_height = self.last_line_height_screen * self.last_rendered_lines as f32;
        let last_width = self.last_char_width_screen * self.chars_per_line as f32;
        let mouse_pos_px = input.get_mouse_pos();
        let mouse_pos_screen = [
            mouse_pos_px[0] / renderer.get_width() as f32,
            mouse_pos_px[1] / renderer.get_height() as f32,
        ];
        let mouse_pos_widget = [
            (mouse_pos_screen[0] - position.offset[0] - padding[0]) / last_width,
            (mouse_pos_screen[1] - position.offset[1] - padding[1]) / last_height,
        ];

        let line_count = self.last_rendered_lines;
        let widget_row = line_count as f32 * mouse_pos_widget[1];
        let screen_row =
            widget_row +
            self.lines_per_screen.min(self.last_rendered_lines) as f32 -
            self.last_rendered_lines as f32;
        let screen_col = self.chars_per_line as f32 * mouse_pos_widget[0];

        if screen_col >= 0.0 && screen_col < self.chars_per_line as f32 && screen_row >= 0.0 && screen_row < self.last_rendered_lines as f32 {
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

                let scroll_scale = 4.0 / char_size_px; // Empirical
                let scroll_delta = input.get_scroll_delta();
                self.ansi_handler.handle_scroll(
                    scroll_delta[0] * scroll_scale,
                    scroll_delta[1] * scroll_scale,
                    flags,
                    &cursor
                );
            }

            self.last_mouse_pos_cell = cursor;
        }

        // Recompute position to incorporate padding
        let mouse_pos_widget = [
            (mouse_pos_screen[0] - position.offset[0]) / (last_width + padding[0] * 2.0),
            (mouse_pos_screen[1] - position.offset[1]) / (last_height + padding[1] * 2.0),
        ];
        self.last_mouse_pos_widget = mouse_pos_widget;
    }

    fn render_background(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        default_height: f32,
        bg_color: &[f32; 4]
    ) {
        renderer.draw_quad(
            &position.offset,
            &[1.0, self.get_last_computed_height_screen().max(default_height)],
            bg_color
        );
    }

    fn render_divider(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        color: &[f32; 4]
    ) {
        let size_px = 2.0;
        let size = size_px / renderer.get_height() as f32;
        renderer.draw_quad(
            &[position.offset[0], position.offset[1]],
            &[1.0, size],
            color
        );
    }

    fn render_terminal(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        char_size_px: f32,
        default_height: f32,
        bg_color: &[f32; 4]
    ) {
        let mut line_offset = 0;
        let mut padding_px = self.padding_px;

        // If the alt screen buf is active, we can ignore special rendering styles
        // Also, ensure the most recent screen is visible (home cursor)
        if self.ansi_handler.is_alt_screen_buf_active() {
            line_offset = self.ansi_handler.get_terminal_state().global_cursor_home[1] as u32;
            padding_px = [0.0, 0.0];
        }

        let width_px = renderer.get_width() as f32 * position.max_size[0];
        let max_chars = ((width_px - padding_px[0] * 2.0) / char_size_px) as u32;
        let rc = renderer.compute_render_constants(position.max_size[0], max_chars);
        let num_screen_lines = renderer.compute_max_lines(&rc, position.max_size[1]);
        let line_size_screen = rc.char_size_y_screen;

        // Cap max render lines based on the alt screen buffer. Here, the state can
        // never be larger than the screen, so never render more than we are supposed
        // to, otherwise dead cells may become visible after resizing
        let max_render_lines = match self.ansi_handler.get_terminal_state().alt_screen_buffer_state {
            BufferState::Active => num_screen_lines,
            _ => 99999999
        };

        if max_chars != self.chars_per_line || num_screen_lines != self.lines_per_screen {
            self.chars_per_line = max_chars;
            self.lines_per_screen = num_screen_lines;

            //log::info!("CPL: {}, LPS: {}", self.chars_per_line, self.lines_per_screen);

            self.ansi_handler.resize(self.chars_per_line, self.lines_per_screen);
        }

        let term_color = [bg_color[0], bg_color[1], bg_color[2]];
        self.ansi_handler.set_terminal_color(&term_color);
        if !self.primary {
            self.ansi_handler.hide_cursor();
        }

        let padding_screen = [padding_px[0] / renderer.get_width() as f32, padding_px[1] / renderer.get_height() as f32];
        let render_offset = [
            position.offset[0] + padding_screen[0],
            position.offset[1] + padding_screen[1]
        ];
        let render_size = [
            position.max_size[0] - 2.0 * padding_screen[0],
            position.max_size[1] - 2.0 * padding_screen[1],
        ];

        let render_stats = renderer.render_terminal(
            &self.ansi_handler.get_terminal_state(),
            &render_size,
            &render_offset,
            max_chars,
            max_render_lines,
            line_offset,
            self.wrap,
            self.debug_line_number,
            self.debug_char_number,
            self.debug_show_cursor
        );

        let clamped_height = (
            render_stats.rendered_line_count as f32 * line_size_screen
        )
         .max(default_height);

        // Set the rendered lines based on the height rather than the actual amount of lines
        self.last_rendered_lines = (clamped_height / line_size_screen).ceil() as u32;
        self.last_line_height_screen = line_size_screen;

        self.last_char_width_screen = rc.char_size_x_screen;

        self.last_computed_height_screen = self.last_rendered_lines as f32 * line_size_screen + padding_screen[1] * 2.0;

        self.last_render_stats = render_stats;
    }

    // Returns true if a rerender should occur after this one, i.e. when a button is pressed
    fn render_overlay(
        &mut self,
        input: &Input,
        renderer: &mut Renderer,
        position: &LayoutPosition,
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
                    position.offset[1] * renderer.get_height() as f32 + self.overlay_padding_px[1]
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
