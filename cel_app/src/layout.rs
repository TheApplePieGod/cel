use cel_renderer::renderer::Renderer;

use crate::terminal_context::TerminalContext;
use crate::input::Input;

// All fields are in screen position
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2],
}

pub struct Layout {
    context: TerminalContext
}

impl Layout {
    pub fn new() -> Self {
        Self {
            context: TerminalContext::new()
        }
    }

    pub fn update(&mut self, input: &Input) {
        self.context.update(input);
    }

    pub fn render(&mut self, renderer: &mut Renderer) {
        let height_px = renderer.get_pixel_height() as f32;
        let size_each_px = 200.0;
        let size_each = size_each_px / height_px;
        let mut cur_offset = 0.0;
        for ctx in self.context.get_widgets() {
            ctx.render(
                renderer,
                &LayoutPosition {
                    offset: [0.0, cur_offset],
                    max_size: [1.0, size_each],
                }
            );

            cur_offset -= size_each;
        }
    }
}
