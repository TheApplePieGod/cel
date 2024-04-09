use cel_renderer::renderer::Renderer;

use crate::terminal_context::TerminalContext;
use crate::input::Input;
use crate::terminal_widget::TerminalWidget;

// All fields are in screen position
#[derive(Copy, Clone)]
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2],
}

pub struct Layout {
    width: u32,
    height: u32,
    can_scroll_up: bool,
    scroll_offset: f32,
    context: TerminalContext,

    widget_height_px: f32,
    widget_gap_px: f32,
}

impl Layout {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width: width as u32,
            height: height as u32,
            can_scroll_up: false,
            scroll_offset: 0.0,
            context: TerminalContext::new(),

            widget_height_px: 54.0,
            widget_gap_px: 3.0,
        }
    }

    pub fn update(&mut self, input: &Input) -> bool {
        let mut any_event = false;

        any_event |= self.context.update(input);

        if self.context.just_split() {
            self.scroll_offset = 0.0;
        }

        // Update scroll
        //let speed_factor = 1.0;
        let speed_factor = 0.01;
        let scroll = input.get_scroll_delta()[1];
        if scroll < 0.0 || self.can_scroll_up {
            if scroll < 0.0 {
                any_event |= true;
            }
            self.scroll_offset = (self.scroll_offset - scroll * speed_factor).min(0.0);
        }

        any_event
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        bg_color: Option<[f32; 3]>,
        renderer: &mut Renderer,
        input: &Input
    ) -> bool {
        let widget_height = self.widget_height_px / self.height as f32;

        let mut should_rerender = false;
        let mut last_local_offset = 0.0;
        let mut last_global_offset = 0.0;
        self.map_onscreen_widgets(|ctx, local_offset, global_offset| {
            let max_size = match ctx.get_expanded() {
                true => 9999999.0,
                false => widget_height
            };

            // Render terminal widget
            if !ctx.get_primary() {
                last_local_offset = local_offset;
                last_global_offset = global_offset;
            }
            should_rerender |= ctx.render(
                renderer,
                input,
                &LayoutPosition {
                    offset: [0.0, local_offset],
                    max_size: [1.0, max_size],
                },
                widget_height,
                bg_color
            );
        });

        // Lock scrolling to the last widget
        self.can_scroll_up = last_local_offset < 0.0;

        should_rerender
    }

    pub fn on_window_resized(&mut self, new_size: [i32; 2]) {
        self.width = new_size[0] as u32;
        self.height = new_size[1] as u32;
    }

    fn get_aspect_ratio(&self) -> f32 { self.width as f32 / self.height as f32 }

    fn map_onscreen_widgets(
        &mut self,
        mut func: impl FnMut(&mut TerminalWidget, f32, f32)
    ) {
        // Draw visible widgets except the primary
        let widget_gap = self.widget_gap_px / self.height as f32;
        let mut cur_offset = 1.0;
        for ctx in self.context.get_widgets().iter_mut().rev() {
            if ctx.get_closed() || ctx.is_empty() {
                continue;
            }

            let last_height = ctx.get_last_computed_height();
            let start_offset = cur_offset - self.scroll_offset - last_height;

            if !ctx.get_primary() {
                func(ctx, start_offset, cur_offset);
            }

            let end_offset = start_offset + last_height;
            if end_offset <= 0.0 {
                break;
            }

            cur_offset -= last_height + widget_gap;
        }

        // Last (primary) widget is always rendered at the bottom
        let last_widget = self.context.get_widgets().last_mut().unwrap();
        func(
            last_widget,
            1.0 - last_widget.get_last_computed_height(),
            1.0
        );
    }
}
