use cel_renderer::renderer::{Coord, Renderer};

use crate::input::Input;

use super::traits::*;

pub struct Button<'a> {
    size: Coord,
    offset: Coord,
    fg_color: [f32; 3],
    bg_color: Option<[f32; 4]>,
    rounding_px: f32,
    char_height_px: f32,
    text: Option<&'a str>
}

impl<'a> Sizable for Button<'a> {
    fn set_size(&mut self, size: Coord) {
        self.size = size;
    }

    fn get_size(&self) -> &Coord {
        &self.size
    }
}

impl<'a> Offsetable for Button<'a> {
    fn set_offset(&mut self, offset: Coord) {
        self.offset = offset;
    }

    fn get_offset(&self) -> &Coord {
        &self.offset
    }
}

impl<'a> Renderable for Button<'a> {
    fn render(&self, renderer: &mut Renderer) {
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
    }
}

// Immediate mode button
impl<'a> Button<'a> {
    pub fn new() -> Self {
        Self {
            size: Coord::Px([0.0, 0.0]),
            offset: Coord::Px([0.0, 0.0]),
            fg_color: [1.0, 1.0, 1.0],
            bg_color: None,
            rounding_px: 0.0,
            char_height_px: 12.0,
            text: None
        }
    }

    pub fn size(mut self, size: Coord) -> Self {
        Sizable::set_size(&mut self, size);
        self
    }

    pub fn offset(mut self, offset: Coord) -> Self {
        Offsetable::set_offset(&mut self, offset);
        self
    }

    pub fn render(self, renderer: &mut Renderer) -> Self {
        Renderable::render(&self, renderer);
        self
    }

    pub fn fg_color(mut self, color: [f32; 3]) -> Self {
        self.fg_color = color;
        self
    }

    pub fn bg_color(mut self, color: [f32; 4]) -> Self {
        self.bg_color = Some(color);
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

    // ----------------------------------

    pub fn is_hovered(&self, renderer: &Renderer, input: &Input) -> bool {
        // TODO: store screen mouse in input?
        let [x, y] = input.get_mouse_pos();

        let offset = self.offset.px(renderer);
        let size = self.size.px(renderer);

        x >= offset[0] && x <= offset[0] + size[0] &&
        y >= offset[1] && y <= offset[1] + size[1]
    }

    pub fn is_clicked(&self, renderer: &Renderer, input: &Input, button: glfw::MouseButton) -> bool {
        self.is_hovered(renderer, input) && input.get_mouse_just_released(button)
    }
}
