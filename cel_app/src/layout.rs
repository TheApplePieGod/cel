use cel_renderer::renderer::Renderer;

use crate::terminal_context::TerminalContext;
use crate::input::Input;

// All fields are in screen position
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2]
}

pub struct Layout {
    contexts: Vec<TerminalContext>
}

impl Layout {
    pub fn new() -> Self {
        Self {
            contexts: vec![TerminalContext::new()]
        }
    }

    pub fn update(&mut self, input: &Input) {
        for ctx in self.contexts.iter_mut() {
            ctx.update(input);
        }
    }

    pub fn render(&mut self, renderer: &mut Renderer) {
        for ctx in self.contexts.iter_mut() {
            ctx.render(
                renderer,
                &LayoutPosition {
                    offset: [0.0, 0.0],
                    max_size: [0.0, 0.0]
                }
            );
        }
    }

    pub fn on_window_resized(&mut self, new_size: [i32; 2]) {
        for ctx in self.contexts.iter_mut() {
            ctx.on_window_resized(new_size);
        }
    }
}
