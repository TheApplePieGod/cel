use cel_renderer::renderer::{Coord, Renderer};

use crate::input::Input;

pub struct Button<'a> {
    size: Coord,
    offset: Coord,
    fg_color: [f32; 3],
    bg_color: [f32; 4],
    rounding_px: f32,
    char_height_px: f32,
    text: Option<&'a str>
}

// Immediate mode button
impl<'a> Button<'a> {
    pub fn new() -> Self {
        Self {
            size: Coord::Px([0.0, 0.0]),
            offset: Coord::Px([0.0, 0.0]),
            fg_color: [1.0, 1.0, 1.0],
            bg_color: [0.0, 0.0, 0.0, 0.0],
            rounding_px: 0.0,
            char_height_px: 12.0,
            text: None
        }
    }

    pub fn size(mut self, size: Coord) -> Self {
        self.size = size;
        self
    }

    pub fn offset(mut self, offset: Coord) -> Self {
        self.offset = offset;
        self
    }

    pub fn fg_color(mut self, color: [f32; 3]) -> Self {
        self.fg_color = color;
        self
    }

    pub fn bg_color(mut self, color: [f32; 4]) -> Self {
        self.bg_color = color;
        self
    }

    pub fn rounding_px(mut self, rounding: f32) -> Self {
        self.rounding_px = rounding;
        self
    }

    pub fn char_height_px(mut self, height: f32) -> Self {
        self.char_height_px = height;
        self
    }

    pub fn text(mut self, text: &'a str) -> Self {
        self.text = Some(text);
        self
    }

    pub fn render(self, renderer: &mut Renderer) -> Self {
        renderer.draw_text(
            self.char_height_px,
            &self.offset,
            &self.size,
            &self.fg_color,
            &self.bg_color,
            true,
            self.rounding_px,
            self.text.unwrap_or("")
        );

        self
    }

    // ----------------------------------

    pub fn is_hovered(&self, renderer: &Renderer, input: &Input) -> bool {
        // TODO: store screen mouse in input?
        let [x, y] = input.get_mouse_pos();

        let offset = self.offset.px(renderer);
        let size = self.offset.px(renderer);

        x >= offset[0] && x <= offset[0] + size[0] &&
        y >= offset[1] && y <= offset[1] + size[1]
    }

    pub fn is_clicked(&self, renderer: &Renderer, input: &Input, button: glfw::MouseButton) -> bool {
        self.is_hovered(renderer, input) && input.get_mouse_just_released(button)
    }
}
