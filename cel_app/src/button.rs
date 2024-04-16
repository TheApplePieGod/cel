use cel_renderer::renderer::Renderer;

use crate::input::Input;

// All in pixel space
pub struct Button {
    size: [f32; 2],
    offset: [f32; 2]
}

// Immediate mode button
impl Button {
    // Origin is top left (0, 0), positive y is down
    pub fn new_px(size: [f32; 2], offset: [f32; 2]) -> Self {
        Self {
            size,
            offset
        }
    }

    // Origin is top left (0, 0), positive y is down
    pub fn new_screen(
        screen_size: [f32; 2],
        size: [f32; 2],
        offset: [f32; 2]
    ) -> Self {
        Self {
            size: [size[0] * screen_size[0], size[1] * screen_size[1]],
            offset: [offset[0] * screen_size[0], offset[1] * screen_size[1]]
        }
    }

    pub fn render(
        &self,
        renderer: &mut Renderer,
        fg_color: &[f32; 3],
        bg_color: &[f32; 3],
        text: &str
    ) {
        let width = renderer.get_width() as f32;
        let height = renderer.get_height() as f32;
        renderer.draw_text(
            150,
            &[self.offset[0] / width, self.offset[1] / height],
            &[self.size[0] / width, self.size[1] / height],
            fg_color,
            bg_color,
            true,
            text
        );
    }

    pub fn is_hovered(&self, input: &Input) -> bool {
        let [x, y] = input.get_mouse_pos();

        x >= self.offset[0] && x <= self.offset[0] + self.size[0] &&
        y >= self.offset[1] && y <= self.offset[1] + self.size[1]
    }

    pub fn is_clicked(&self, input: &Input, button: glfw::MouseButton) -> bool {
        self.is_hovered(input) && input.get_mouse_just_released(button)
    }
}
