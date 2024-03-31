use cel_core::ansi::{AnsiHandler, BufferState};
use cel_renderer::renderer::Renderer;
use glfw::MouseButton;

use crate::{button::Button, input::Input, layout::LayoutPosition};

pub struct TerminalWidget {
    char_buffer: Vec<u8>,
    ansi_handler: AnsiHandler,
    line_offset: f32,
    chars_per_line: u32,
    lines_per_screen: u32,
    last_computed_height: f32,

    primary: bool,
    closed: bool,
    expanded: bool,
    wrap: bool,

    char_size_px: f32,
    button_size_px: f32,

    debug_line_number: bool,
    debug_show_cursor: bool
}

impl TerminalWidget {
    pub fn new() -> Self {
        Self {
            char_buffer: vec![],
            ansi_handler: AnsiHandler::new(),
            line_offset: 0.0,
            chars_per_line: 180,
            lines_per_screen: 1,
            last_computed_height: 0.0,

            primary: true,
            closed: false,
            expanded: true,
            wrap: true,

            char_size_px: 8.0,
            button_size_px: 20.0,

            debug_line_number: false,
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
        self.ansi_handler.reset();
    }

    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        input: &Input,
        position: &LayoutPosition,
        default_height: f32,
        bg_color: Option<[f32; 3]>,
    ) {
        let bg_color = bg_color.unwrap_or([0.0, 0.0, 0.0]);

        self.render_background(renderer, position, default_height, &bg_color);
        self.render_divider(renderer, position);
        self.render_terminal(renderer, position, default_height, &bg_color);
        self.render_overlay(input, renderer, position);
    }

    pub fn get_last_computed_height(&self) -> f32 { self.last_computed_height }
    pub fn get_closed(&self) -> bool { self.closed }
    pub fn get_expanded(&self) -> bool { self.expanded }
    pub fn set_expanded(&mut self, expanded: bool) { self.expanded = expanded }
    pub fn get_primary(&self) -> bool { self.primary }
    pub fn set_primary(&mut self, primary: bool) { self.primary = primary }
    pub fn get_terminal_size(&self) -> [u32; 2] { [self.chars_per_line, self.lines_per_screen] }

    fn render_background(
        &mut self,
        renderer: &mut Renderer,
        position: &LayoutPosition,
        default_height: f32,
        bg_color: &[f32; 3]
    ) {
        renderer.draw_quad(
            &position.offset,
            &[1.0, self.get_last_computed_height().max(default_height)],
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
            &[position.offset[0], position.offset[1] - size * 2.0],
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
        let padding_px = 20.0;
        let padding = padding_px / renderer.get_width() as f32;
        let width_px = renderer.get_width() as f32 * position.max_size[0];
        let max_chars = ((width_px - 2.0 * padding_px) / self.char_size_px) as u32;
        let num_screen_lines = renderer.compute_max_lines(max_chars, 1.0);
        let line_size_screen = 1.0 / num_screen_lines as f32;
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

        /*
        let padded_offset = [
            position.offset[0] + padding,
            position.offset[1]
        ];
        */
        let rendered_lines = renderer.render_terminal(
            &self.ansi_handler.get_terminal_state(),
            &position.offset,
            max_chars,
            max_render_lines,
            self.line_offset,
            self.wrap,
            self.debug_line_number,
            self.debug_show_cursor
        );

        //log::warn!("RL: {}", rendered_lines);
        self.last_computed_height = (
            rendered_lines as f32 * line_size_screen
        )
         .max(default_height)
         .min(position.max_size[1]);
    }

    fn render_overlay(
        &mut self,
        input: &Input,
        renderer: &mut Renderer,
        position: &LayoutPosition,
    ) {
        let aspect = renderer.get_aspect_ratio();
        let screen_size = [renderer.get_width() as f32, renderer.get_height() as f32];
        let button_size = [
            self.button_size_px / screen_size[0],
            self.button_size_px / screen_size[0] as f32 * aspect
        ];

        // Close button
        if !self.primary {
            let button = Button::new_screen(
                screen_size,
                button_size,
                [1.0 - button_size[0], position.offset[1]]
            );
            button.render(renderer, &[1.0, 1.0, 1.0], &[0.05, 0.05, 0.1],  "✘");
            if button.is_clicked(input, MouseButton::Button1) {
                self.close();
            }
        }
    }
}
