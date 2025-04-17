use cel_renderer::renderer::{RenderStats, Renderer};

use crate::terminal_context::TerminalContext;
use crate::input::{Input, InputEvent};
use crate::terminal_widget::TerminalWidget;

// All fields are in screen position
#[derive(Copy, Clone)]
pub struct LayoutPosition {
    pub offset: [f32; 2],
    pub max_size: [f32; 2],
}

pub struct Layout {
    width_screen: f32,
    height_screen: f32,
    offset_x_screen: f32,
    offset_y_screen: f32,
    can_scroll_up: bool,
    scroll_offset: f32,
    context: TerminalContext,

    last_fullscreen_state: bool,
    last_num_onscreen_widgets: u32,
    last_accumulated_render_stats: RenderStats,

    char_size_px: f32,
    min_widget_lines: u32,
}

impl Layout {
    pub fn new(
        renderer: &Renderer,
        width_screen: f32,
        height_screen: f32,
        char_size_px: f32,
        cwd: Option<&str>
    ) -> Self {
        // Not perfect due to possible initial padding of the widget, but difference
        // should be negligible so it shouldn't matter
        let max_rows = renderer.get_max_lines(height_screen, char_size_px);
        let max_cols = renderer.get_chars_per_line(width_screen, char_size_px);

        Self {
            width_screen,
            height_screen,
            offset_x_screen: 0.0,
            offset_y_screen: 0.0,

            can_scroll_up: false,
            scroll_offset: 0.0,
            context: TerminalContext::new(max_rows, max_cols, cwd),

            last_fullscreen_state: false,
            last_num_onscreen_widgets: 0,
            last_accumulated_render_stats: Default::default(),

            char_size_px,
            min_widget_lines: 5,
        }
    }

    // Returns (any_event, terminated)
    pub fn update(&mut self, renderer: &Renderer, input: Option<&mut Input>) -> (bool, bool) {
        let mut any_event = false;

        let mut input = input;
        let (ctx_event, ctx_terminated) = self.context.update(input.as_deref_mut());
        any_event |= ctx_event;

        if self.context.just_split() {
            self.scroll_offset = 0.0;
        }

        // Perform a hard resize when the fullscreen state changes. This ensures
        // the terminal context respects the width given any updated padding constraints
        let is_fullscreen = self.context.get_primary_widget().is_fullscreen();
        if self.last_fullscreen_state != is_fullscreen {
            self.hard_resize(renderer);
            self.last_fullscreen_state = is_fullscreen;
        }

        // Handle input events
        if let Some(input) = input {
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

            any_event |= input.consume_event(InputEvent::ZoomIn, || {
                self.char_size_px = (self.char_size_px + 2.0).min(32.0);
                self.hard_resize(renderer);
            });
            any_event |= input.consume_event(InputEvent::ZoomOut, || {
                self.char_size_px = (self.char_size_px - 2.0).max(4.0);
                self.hard_resize(renderer);
            });
        }

        (any_event, ctx_terminated)
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        bg_color: Option<[f32; 4]>,
        divider_color: Option<[f32; 4]>,
        err_bg_color: Option<[f32; 4]>,
        err_divider_color: Option<[f32; 4]>,
        renderer: &mut Renderer,
        input: &Input
    ) -> bool {
        let min_widget_lines = self.min_widget_lines;
        let width_screen = self.width_screen;
        let height_screen = self.height_screen;
        let offset_x_screen = self.offset_x_screen;
        let char_size_px = self.char_size_px;

        let max_cols = renderer.get_chars_per_line(self.width_screen, self.char_size_px);
        let rc = renderer.compute_render_constants(self.width_screen, max_cols);
        let line_size_screen = rc.char_size_y_screen;

        renderer.enable_scissor();
        renderer.update_scissor_screen(
            self.offset_x_screen,
            self.offset_y_screen,
            self.width_screen,
            self.height_screen
        );

        let mut should_rerender = false;
        let mut count = 0;
        let mut min_local_offset: f32 = 1.0;
        let mut accum_stats: RenderStats = Default::default();
        self.map_onscreen_widgets(renderer,  |renderer, ctx, local_offset, height| {
            min_local_offset = min_local_offset.min(local_offset - height);

            let (bg_color, divider_color) = match ctx.get_exit_code() {
                None | Some(0) => (bg_color, divider_color),
                // Error code
                _ => (err_bg_color, err_divider_color)
            };

            ctx.reset_render_state();

            // Render terminal widget
            should_rerender |= ctx.render(
                renderer,
                input,
                &LayoutPosition {
                    offset: [offset_x_screen, local_offset],
                    max_size: [width_screen, height_screen],
                },
                char_size_px,
                min_widget_lines,
                bg_color,
                divider_color
            );

            let stats = ctx.get_last_render_stats();
            accum_stats.num_fg_instances += stats.num_fg_instances;
            accum_stats.num_bg_instances += stats.num_bg_instances;
            accum_stats.wrapped_line_count += stats.wrapped_line_count;
            accum_stats.rendered_line_count += stats.rendered_line_count;

            count += 1;
        });

        renderer.disable_scissor();

        // Lock scrolling to the last widget
        let top = self.offset_y_screen;
        self.can_scroll_up = min_local_offset < top;

        self.last_num_onscreen_widgets = count;
        self.last_accumulated_render_stats = accum_stats;

        should_rerender
    }

    pub fn resize(
        &mut self,
        renderer: &Renderer,
        soft: bool,
        new_size_screen: [f32; 2],
        new_offset_screen: [f32; 2]
    ) {
        self.width_screen = new_size_screen[0];
        self.height_screen = new_size_screen[1];
        self.offset_x_screen = new_offset_screen[0];
        self.offset_y_screen = new_offset_screen[1];

        // Resize context after a hard resize
        if !soft {
            // Ensure maximum size accounts for current widget padding
            let padding = self.context.get_primary_widget().get_padding(renderer);
            let max_rows = renderer.get_max_lines(self.height_screen - padding[1] * 2.0, self.char_size_px);
            let max_cols = renderer.get_chars_per_line(self.width_screen - padding[0] * 2.0, self.char_size_px);

            self.context.resize(max_rows, max_cols);
        }
    }

    pub fn get_char_size_px(&self) -> f32 {
        self.char_size_px
    }

    pub fn get_current_directory(&self) -> &str {
        self.context.get_primary_widget().get_current_dir()
    }

    pub fn get_name(&self) -> &str {
        self.get_current_directory()
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let widgets = self.context.get_widgets();

        let active = widgets.last().unwrap();
        let last_stats = &self.last_accumulated_render_stats;
        let mut text_lines = vec![
            format!("Total widgets: {}", widgets.len()),
            format!("Rendered widgets: {}", self.last_num_onscreen_widgets),
            format!("Total fg quads: {}", last_stats.num_fg_instances),
            format!("Total bg quads: {}", last_stats.num_bg_instances),
            format!("Total rendered lines: {}", last_stats.rendered_line_count),
            format!("Total wrapped lines: {}", last_stats.wrapped_line_count),
            format!("Scroll offset: {}", self.scroll_offset),
            String::from("\n"),
            format!("Active Widget"),
        ];

        text_lines.extend(active.get_debug_lines());

        text_lines
    }

    fn hard_resize(&mut self, renderer: &Renderer) {
        self.resize(renderer, false, [self.width_screen, self.height_screen], [self.offset_x_screen, self.offset_y_screen]);
    }

    // Use dummy ctx to compute the minimum widget height, max rows, max cols
    // TODO: this is not great
    fn get_dummy_ctx_params(&self, renderer: &Renderer) -> (f32, u32, u32) {
        let dummy_ctx = TerminalWidget::new(0, 0);
        let padding = dummy_ctx.get_padding(renderer);
        let height = dummy_ctx.get_height_screen(renderer, self.width_screen, 1.0, self.char_size_px, self.min_widget_lines);
        let max_rows = renderer.get_max_lines(self.height_screen - padding[1] * 2.0, self.char_size_px);
        let max_cols = renderer.get_chars_per_line(self.width_screen - padding[0] * 2.0, self.char_size_px);
        (height, max_rows, max_cols)
    }

    fn map_onscreen_widgets(
        &mut self,
        renderer: &mut Renderer,
        mut func: impl FnMut(&mut Renderer, &mut TerminalWidget, f32, f32)
    ) {
        let primary_fullscreen = self.context.get_primary_widget().is_fullscreen();
        let scroll_offset = match primary_fullscreen {
            true => 0.0,
            false => self.scroll_offset
        };

        // Draw visible widgets except the primary
        let top = self.offset_y_screen;
        let bottom = self.height_screen + self.offset_y_screen;
        let mut cur_offset = bottom;
        for ctx in self.context.get_widgets_mut().iter_mut().rev() {
            // Skip processing of non-primary if fullscreen
            if !ctx.get_primary() && primary_fullscreen {
                continue;
            }

            if (ctx.is_empty() && !ctx.get_primary()) || ctx.get_closed() {
                continue;
            }

            // Stop once the start offset is above the top of the layout
            let start_offset = cur_offset - scroll_offset;
            if start_offset < top {
                break;
            }

            // Only render if not primary (handled later) and actually visible on screen
            let ctx_height_pre = ctx.get_height_screen(renderer, self.width_screen, start_offset, self.char_size_px, self.min_widget_lines);
            if !ctx.get_primary() && start_offset - ctx_height_pre < bottom {
                func(renderer, ctx, start_offset, ctx_height_pre);

                // Do some math to determine if the height changed during render. If this happens,
                // it means the widget was reflowed and has a different height now. In order to maintain
                // visual consistency, adjust the scroll offset to reflect this difference iff the widget is
                // not at the top of the screen (i.e. when there are widgets above that would be affected by the height
                // difference). This approach lets us perform the expensive reflow in a deferred manner only
                // when the widget is rendered rather than doing them all at once when the screen is resized.
                let ctx_height_post = ctx.get_height_screen(renderer, self.width_screen, start_offset, self.char_size_px, self.min_widget_lines);
                let height_diff = ctx_height_post - ctx_height_pre;
                let is_not_top_widget = start_offset - ctx_height_pre > top;
                if height_diff != 0.0 && is_not_top_widget {
                    self.scroll_offset = (self.scroll_offset - height_diff).min(0.0);
                }
            }

            cur_offset -= ctx_height_pre;
        }

        // Last (primary) widget is always rendered at the bottom
        // It should snap to the bottom of the last widget when scrolling
        // such that it grows when scrolling down and shrinks when scrolling up,
        // up to a minimum size

        let (min_height, _, _) = self.get_dummy_ctx_params(renderer);
        let primary_ctx = self.context.get_primary_widget_mut();
        let primary_height = primary_ctx.get_height_screen(renderer, self.width_screen, bottom, self.char_size_px, self.min_widget_lines);
        let start_offset = (bottom - scroll_offset).min(bottom + primary_height - min_height);
        func(renderer, primary_ctx, start_offset, primary_height);
    }
}
