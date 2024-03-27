use cel_renderer::renderer::Renderer;

use crate::input::Input;

// All in pixel space
pub struct Button {
    size: [f32; 2],
    offset: [f32; 2]
}

impl Button {
    // Origin is top left (0, 0), positive y is down
    pub fn new_px(size: [f32; 2], offset: [f32; 2]) -> Self {
        Self {
            size,
            offset
        }
    }

    // Origin is top left (0, 0), positive y is up
    pub fn new_screen(
        screen_size: [f32; 2],
        size: [f32; 2],
        offset: [f32; 2]
    ) -> Self {
        Self {
            size: [size[0] * screen_size[0], size[1] * screen_size[1]],
            offset: [offset[0] * screen_size[0], -offset[1] * screen_size[1]]
        }
    }

    pub fn render(&self, renderer: &mut Renderer, text: &str) {
        /*
        renderer.draw_text(
            &[1.0 - button_size, local_offset],
            &[button_size, button_size * aspect],
            &[1.0, 1.0, 1.0],
            &[0.05, 0.05, 0.1],
            "✘"
        );
        */
    }

    pub fn is_hovered(&self, input: &Input) -> bool {
        let [x, y] = input.get_mouse_pos();

        x >= self.offset[0] && x <= self.offset[0] + self.size[0] &&
        y >= self.offset[1] && y <= self.offset[1] + self.size[1]
    }
}
