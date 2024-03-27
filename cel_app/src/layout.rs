use cel_renderer::renderer::Renderer;

use crate::button::Button;
use crate::terminal_context::TerminalContext;
use crate::input::Input;
use crate::terminal_widget::TerminalWidget;

// All fields are in screen position
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2],
}

pub struct Layout {
    width: u32,
    height: u32,
    can_scroll_down: bool,
    scroll_offset: f32,
    context: TerminalContext,

    widget_height_px: f32,
    widget_gap_px: f32,
    button_size_px: f32,
}

impl Layout {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width: width as u32,
            height: height as u32,
            can_scroll_down: false,
            scroll_offset: 0.0,
            context: TerminalContext::new(),

            widget_height_px: 100.0,
            widget_gap_px: 10.0,
            button_size_px: 15.0,
        }
    }

    pub fn update(&mut self, input: &Input) {
        self.context.update(input);

        let aspect = self.get_aspect();
        let screen_size = [self.width as f32, self.height as f32];
        let button_size = [
            self.button_size_px / self.width as f32,
            self.button_size_px / self.width as f32 * aspect
        ];
        self.map_onscreen_widgets(|_, local_offset, _| {
            // Close 
            let button = Button::new_screen(
                screen_size,
                button_size,
                [1.0 - button_size[0], local_offset]
            );
            let hovered = button.is_hovered(input);
            //log::warn!("{}", hovered);
        });

        //let speed_factor = 1.0;
        let speed_factor = 0.01;
        let scroll = input.get_scroll_delta()[1];
        if scroll > 0.0 || self.can_scroll_down {
            self.scroll_offset = (self.scroll_offset - scroll * speed_factor).max(0.0)   ;
        }
    }

    pub fn render(&mut self, renderer: &mut Renderer) {
        let bg_color: [f32; 3] = [0.1, 0.1, 0.2];
        let widget_height = self.widget_height_px / self.height as f32;

        let aspect = self.get_aspect();
        let screen_size = [self.width as f32, self.height as f32];
        let button_size = [
            self.button_size_px / self.width as f32,
            self.button_size_px / self.width as f32 * aspect
        ];
        let mut rendered_count = 0;
        let mut last_offset = 0.0;
        self.map_onscreen_widgets(|ctx, local_offset, global_offset| {
            let max_size = match ctx.get_expanded() {
                true => 1.0,
                false => widget_height
            };

            // Draw background
            renderer.draw_quad(
                &[0.0, local_offset],
                &[1.0, ctx.get_last_computed_height().max(widget_height)],
                &bg_color
            );

            // Render terminal widget
            last_offset = global_offset;
            rendered_count += 1;
            ctx.render(
                renderer,
                &LayoutPosition {
                    offset: [0.0, local_offset],
                    max_size: [1.0, max_size],
                },
                Some(bg_color)
            );

            // Overlay buttons

            let button = Button::new_screen(
                screen_size,
                button_size,
                [1.0 - button_size[0], local_offset]
            );
            //button.render(renderer, "✘")
        });

        // Lock scrolling to the last widget
        self.scroll_offset = self.scroll_offset.min(last_offset);
        self.can_scroll_down = rendered_count > 1;
    }

    pub fn on_window_resized(&mut self, new_size: [i32; 2]) {
        self.width = new_size[0] as u32;
        self.height = new_size[1] as u32;
    }

    fn get_aspect(&self) -> f32 { self.width as f32 / self.height as f32 }

    fn map_onscreen_widgets(
        &mut self,
        mut func: impl FnMut(&mut TerminalWidget, f32, f32)
    ) {
        // Always render the last widget if no widgets are visible

        let widget_count = self.context.get_widgets().len();
        let widget_gap = self.widget_gap_px / self.height as f32;
        let mut rendered_count = 0;
        let mut cur_offset = 0.0;
        for (i, ctx) in self.context.get_widgets().iter_mut().enumerate() {
            let last_height = ctx.get_last_computed_height();
            let start_offset = cur_offset - self.scroll_offset;
            let end_offset = start_offset + last_height;
            let is_visible = start_offset >= 0.0 || end_offset >= 0.0;
            let is_last = rendered_count == 0 && i == widget_count - 1;
            if is_visible || is_last {
                func(ctx, start_offset, cur_offset);
                rendered_count += 1;
            }

            if end_offset >= 1.0 {
                break;
            }

            cur_offset += last_height + widget_gap;
        }
    }
}
