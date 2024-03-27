use cel_renderer::renderer::Renderer;

use crate::terminal_context::TerminalContext;
use crate::input::Input;
use crate::terminal_widget::TerminalWidget;

// All fields are in screen position
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2],
}

pub struct Layout {
    scroll_offset: f32,
    context: TerminalContext
}

impl Layout {
    pub fn new() -> Self {
        Self {
            scroll_offset: 0.0,
            context: TerminalContext::new()
        }
    }

    pub fn update(&mut self, input: &Input) {
        self.context.update(input);

        let speed_factor = 0.01;
        let scroll = input.get_scroll_delta();
        self.scroll_offset -= scroll[1] * speed_factor;
    }

    pub fn render(&mut self, renderer: &mut Renderer) {
        let height_px = renderer.get_pixel_height() as f32;
        let size_each_px = 200.0;
        let size_each = size_each_px / height_px;

        let mut idx = 0;
        self.map_onscreen_widgets(|ctx, offset| {
            let max_size = match ctx.get_expanded() {
                true => 1.0,
                false => size_each
            };

            idx += 1;
            ctx.render(
                renderer,
                &LayoutPosition {
                    offset: [0.0, offset],
                    max_size: [1.0, max_size],
                }
            );
        });
    }

    fn map_onscreen_widgets(&mut self, mut func: impl FnMut(&mut TerminalWidget, f32)) {
        let mut cur_offset = 0.0;
        for ctx in self.context.get_widgets() {
            let last_height = ctx.get_last_computed_height();
            let start_offset = cur_offset - self.scroll_offset;
            let end_offset = start_offset + last_height;
            if start_offset >= 0.0 || end_offset >= 0.0 {
                func(ctx, -start_offset);
            }

            if end_offset >= 1.0 {
                break;
            }

            cur_offset += last_height;
        }
    }
}
