use cel_core::ansi::{self, AnsiHandler, BufferState};
use cel_renderer::renderer::Renderer;

use crate::{button::Button, input::Input, layout::LayoutPosition};

const MOUSE_BUTTON_MAPPING: [(ansi::MouseButton, glfw::MouseButton); 3] = [
    (ansi::MouseButton::Mouse1, glfw::MouseButton::Button1),
    (ansi::MouseButton::Mouse2, glfw::MouseButton::Button2),
    (ansi::MouseButton::Mouse3, glfw::MouseButton::Button3)
];

pub struct TerminalWidget {
    char_buffer: Vec<u8>,
    ansi_handler: AnsiHandler,
    chars_per_line: u32,
    lines_per_screen: u32,
    last_computed_height: f32,
    last_rendered_lines: u32,
    last_line_height_screen: f32,
    last_char_width_screen: f32,

    primary: bool,
    closed: bool,
    expanded: bool,
    wrap: bool,

    padding_px: [f32; 2],
    char_size_px: f32,
    button_size_px: f32,

    debug_line_number: bool,
    debug_char_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new() -> Self {
        let chars_per_line = 180; // Sane default size
        let lines_per_screen = 40;

        Self {
            char_buffer: vec![],
            ansi_handler: AnsiHandler::new(chars_per_line, lines_per_screen),
            chars_per_line,
            lines_per_screen,
            last_computed_height: 0.0,
            last_rendered_lines: 0,
            last_line_height_screen: 1.0, // To prevent the 'excess space' from blowing up on initial render
            last_char_width_screen: 0.0,

            primary: true,
            closed: false,
            expanded: true,
            wrap: true,

            padding_px: [12.0, 12.0],
            char_size_px: 8.0,
            button_size_px: 20.0,

            debug_line_number: false,
            debug_char_number: false,
            debug_show_cursor: false
        }
    }

    pub fn consume_output_stream(&mut self) -> Vec<u8> {
        self.ansi_handler.consume_output_stream()
    }

    pub fn push_chars(&mut self, chars: &[u8], stop_early: bool) -> Option<u32> {
        //self.char_buffer.push(char);
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

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        input: &Input,
        position: &LayoutPosition,
        default_height: f32,
        bg_color: Option<[f32; 3]>,
    ) -> bool {
        // Align the widget such that the first line is at the top of the screen, rather
        // than the bottom always being at the bottom if the lines do not fully fill up
        // the screen space
        let excess_space = 1.0 - (self.last_line_height_screen * self.lines_per_screen as f32);
        let mut real_position = *position;
        real_position.offset[1] -= excess_space;

        self.update_mouse_input(renderer, input, &real_position);

        let bg_color = bg_color.unwrap_or([0.0, 0.0, 0.0]);
        self.render_background(renderer, &real_position, default_height, excess_space, &bg_color);
        self.render_divider(renderer, &real_position);
        self.render_terminal(renderer, &real_position, default_height, &bg_color);

        self.render_overlay(input, renderer, &real_position)
    }

    pub fn is_empty(&self) -> bool { self.ansi_handler.is_empty() }
    pub fn get_last_computed_height(&self) -> f32 { self.last_computed_height }
    pub fn get_closed(&self) -> bool { self.closed }
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
        position: &LayoutPosition
    ) {
        if !self.primary {
            return;
        }

        // Compute the target cell based on the mouse position and widget position

        let last_height = self.last_computed_height;
        let last_width = self.last_char_width_screen * self.chars_per_line as f32;
        let mouse_pos_px = input.get_mouse_pos();
        let mouse_pos_screen = [
            mouse_pos_px[0] / renderer.get_width() as f32,
            mouse_pos_px[1] / renderer.get_height() as f32,
        ];
        let mouse_pos_widget = [
            (mouse_pos_screen[0] - position.offset[0]) / last_width,
            (mouse_pos_screen[1] - position.offset[1]) / last_height,
        ];

        let line_count = self.last_rendered_lines;
        let widget_row = (line_count as f32 * mouse_pos_widget[1]) as i32;
        let screen_row =
            widget_row +
            self.lines_per_screen.min(self.last_rendered_lines) as i32 -
            self.last_rendered_lines as i32;

        if screen_row < 0 || screen_row >= self.lines_per_screen as i32 {
            return;
        }

        let screen_col = (self.chars_per_line as f32 * mouse_pos_widget[0]) as i32;
        if screen_col < 0 || screen_col >= self.chars_per_line as i32 {
            return;
        }

        let cursor = [screen_col as usize, screen_row as usize];
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

        for entry in MOUSE_BUTTON_MAPPING {
            self.ansi_handler.handle_mouse_button(
                entry.0,
                input.get_mouse_down(entry.1),
                flags,
                &cursor
            );
        }

        let scroll_scale = 4.0 / self.char_size_px; // Empirical
        let scroll_delta = input.get_scroll_delta();
        self.ansi_handler.handle_scroll(
            scroll_delta[0] * scroll_scale,
            scroll_delta[1] * scroll_scale,
            flags,
            &cursor
        );
    }

    fn render_background(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        default_height: f32,
        excess_space: f32,
        bg_color: &[f32; 3]
    ) {
        // Increase the size of the primary widget to compensate for the upward
        // alignment shift so that scrolled widgets are not visible behind the primary
        let extra_height = match self.primary {
            true => excess_space,
            false => 0.0
        };

        renderer.draw_quad(
            &position.offset,
            &[1.0, self.get_last_computed_height().max(default_height) + extra_height],
            &bg_color
        );
    }

    fn render_divider(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition
    ) {
        let size_px = 1.0;
        let size = size_px / renderer.get_height() as f32;
        renderer.draw_quad(
            &[position.offset[0], position.offset[1]],
            &[1.0, size],
            &[0.933, 0.388, 0.321]
        );
    }

    fn render_terminal(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        default_height: f32,
        bg_color: &[f32; 3]
    ) {
        let mut line_offset = 0.0;
        let mut padding_px = self.padding_px;

        // If the alt screen buf is active, we can ignore special rendering styles
        // Also, ensure the most recent screen is visible (home cursor)
        if self.ansi_handler.is_alt_screen_buf_active() {
            line_offset = self.ansi_handler.get_terminal_state().global_cursor_home[1] as f32;
            padding_px = [0.0, 0.0];
        }

        let padding = [padding_px[0] / renderer.get_width() as f32, padding_px[1] / renderer.get_height() as f32];
        let width_px = renderer.get_width() as f32 * position.max_size[0];
        let max_chars = ((width_px - padding_px[0] * 2.0) / self.char_size_px) as u32;
        let rc = renderer.compute_render_constants(max_chars, &self.padding_px);
        let num_screen_lines = renderer.compute_max_lines(&rc, 1.0);
        let line_size_screen = rc.char_size_y_screen * rc.line_height;
        let num_actual_lines = (position.max_size[1] / line_size_screen) as u32;
        let max_terminal_lines = num_screen_lines.min(num_actual_lines);

        // Cap max render lines based on the alt screen buffer. Here, the state can
        // never be larger than the screen, so never render more than we are supposed
        // to, otherwise dead cells may become visible after resizing
        let max_render_lines = match self.ansi_handler.get_terminal_state().alt_screen_buffer_state {
            BufferState::Active => max_terminal_lines,
            _ => num_actual_lines
        };

        if max_chars != self.chars_per_line || max_terminal_lines != self.lines_per_screen {
            self.chars_per_line = max_chars;
            self.lines_per_screen = max_terminal_lines;

            //log::info!("CPL: {}, LPS: {}", self.chars_per_line, self.lines_per_screen);

            self.ansi_handler.resize(self.chars_per_line, self.lines_per_screen);
        }

        self.ansi_handler.set_terminal_color(&bg_color);
        if !self.primary {
            self.ansi_handler.hide_cursor();
        }

        let padded_offset = [
            position.offset[0] + padding[0],
            position.offset[1] + padding[1]
        ];
        let rendered_lines = renderer.render_terminal(
            &self.ansi_handler.get_terminal_state(),
            &padded_offset,
            &self.padding_px,
            max_chars,
            max_render_lines,
            line_offset,
            self.wrap,
            self.debug_line_number,
            self.debug_char_number,
            self.debug_show_cursor
        );

        //log::warn!("RL: {}", rendered_lines);
        let clamped_height = (
            rendered_lines as f32 * line_size_screen
        )
         .max(default_height)
         .min(position.max_size[1]);

        // Set the rendered lines based on the height rather than the actual amount of lines
        self.last_rendered_lines = (clamped_height / line_size_screen).ceil() as u32;
        self.last_line_height_screen = line_size_screen;

        self.last_char_width_screen = rc.char_root_size * rc.char_size_x_screen;

        self.last_computed_height = self.last_rendered_lines as f32 * line_size_screen + padding[1] * 2.0;
    }

    // Returns true if a rerender should occur after this one, i.e. when a button is pressed
    fn render_overlay(
        &mut self,
        input: &Input,
        renderer: &mut Renderer,
        position: &LayoutPosition,
    ) -> bool {
        let aspect = renderer.get_aspect_ratio();
        let screen_size = [renderer.get_width() as f32, renderer.get_height() as f32];
        let button_size = [
            self.button_size_px / screen_size[0],
            self.button_size_px / screen_size[0] as f32 * aspect
        ];

        let mut should_rerender = false;

        // Close button
        if !self.primary {
            let button = Button::new_screen(
                screen_size,
                button_size,
                [1.0 - button_size[0], position.offset[1]]
            );
            button.render(renderer, &[1.0, 1.0, 1.0], &[0.05, 0.05, 0.1],  "✘");
            if button.is_clicked(input, glfw::MouseButton::Button1) {
                self.close();
                should_rerender = true;
            }
        }

        should_rerender
    }
}
