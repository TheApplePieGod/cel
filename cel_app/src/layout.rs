use cel_renderer::renderer::{self, RenderConstants, RenderStats, Renderer};

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
    position: LayoutPosition,
    can_scroll_up: bool,
    scroll_offset: f32,
    context: TerminalContext,

    last_command_running_state: bool,
    last_fullscreen_state: bool,
    last_num_onscreen_widgets: u32,
    last_accumulated_render_stats: RenderStats,

    char_size_px: f32,
    default_char_size_px: f32,
    min_widget_lines: u32,
}

impl Layout {
    pub fn new(
        renderer: &Renderer,
        width_screen: f32,
        height_screen: f32,
        char_size_px: f32,
        default_char_size_px: f32,
        cwd: Option<&str>
    ) -> Self {
        // Approximation, but should be fine for the initial size (does not account for padding)
        let rc = renderer.compute_render_constants(width_screen, height_screen, char_size_px);

        Self {
            position: LayoutPosition {
                offset: [0.0, 0.0],
                max_size: [width_screen, height_screen]
            },

            can_scroll_up: false,
            scroll_offset: 0.0,
            context: TerminalContext::new(rc.max_rows, rc.max_cols, cwd),

            last_command_running_state: false,
            last_fullscreen_state: false,
            last_num_onscreen_widgets: 0,
            last_accumulated_render_stats: Default::default(),

            char_size_px,
            default_char_size_px,
            min_widget_lines: 5,
        }
    }

    // Returns (any_event, terminated)
    pub fn update(&mut self, renderer: &Renderer, input: Option<&mut Input>) -> (bool, bool) {
        let mut any_event = false;

        let mut input = input;
        let old_primary_lines = self.context.get_primary_widget().get_num_physical_lines();
        let (ctx_event, ctx_terminated) = self.context.update(input.as_deref_mut());
        any_event |= ctx_event;

        // Update the scroll offset when the primary widget is outputting new lines. This way,
        // if the user is scrolled up, the layout does not visibly shift.
        let new_primary_lines = self.context.get_primary_widget().get_num_physical_lines();
        if new_primary_lines > old_primary_lines && self.scroll_offset <= -1.0 {
            self.scroll_offset -= (new_primary_lines - old_primary_lines) as f32;
        }

        // On certain actions, reset the scroll offset to the bottom of the screen
        let command_running_state = self.context.get_primary_widget().is_command_running();
        if self.context.just_split() || command_running_state != self.last_command_running_state {
            self.scroll_offset = 0.0;
            self.last_command_running_state = command_running_state;
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
            let speed_factor = 1.0; // Line space
            let scroll = input.get_scroll_delta()[1];
            if scroll < 0.0 || self.can_scroll_up {
                if scroll < 0.0 {
                    any_event |= true;
                }
                self.scroll_offset = (self.scroll_offset - scroll * speed_factor).min(0.0);
            }

            any_event |= input.consume_event(InputEvent::ZoomReset, || {
                self.char_size_px = self.default_char_size_px;
                self.hard_resize(renderer);
            });
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
        input: &mut Input
    ) -> bool {
        let min_widget_lines = self.min_widget_lines;
        let char_size_px = self.char_size_px;
        let position = self.position;

        renderer.enable_scissor();
        renderer.update_scissor_screen(
            self.position.offset[0],
            self.position.offset[1],
            self.position.max_size[0],
            self.position.max_size[1]
        );

        let mut should_rerender = false;
        let mut count = 0;
        let mut min_local_offset: f32 = 1.0;
        let mut accum_stats: RenderStats = Default::default();
        self.map_onscreen_widgets(renderer,  |renderer, ctx, local_offset, height| {
            min_local_offset = min_local_offset.min(local_offset - height);

            let (bg_color, divider_color) = match ctx.get_exit_code() {
                // Success or CTRL+C
                None | Some(0) | Some(130) => (bg_color, divider_color),
                // Error code
                _ => (err_bg_color, err_divider_color)
            };

            ctx.reset_render_state();

            // Render terminal widget
            should_rerender |= ctx.render(
                renderer,
                input,
                &LayoutPosition {
                    offset: [position.offset[0], local_offset],
                    max_size: position.max_size
                },
                &position,
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

        // After we've rendered all widgets, consume the copy event to make sure
        // it doesn't stick around if no widgets consumed it
        input.consume_event(InputEvent::Copy, || {});

        // Lock scrolling to the last widget
        let top = position.offset[1];
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
        self.position = LayoutPosition {
            offset: new_offset_screen,
            max_size: new_size_screen
        };

        // Resize context after a hard resize
        if !soft {
            // Ensure maximum size accounts for current widget padding
            let rc = self.context.get_primary_widget().get_render_constants(
                renderer,
                self.position.max_size[0],
                self.position.max_size[1],
                self.char_size_px
            );
            self.context.resize(rc.max_rows, rc.max_cols);
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
        self.resize(renderer, false, self.position.max_size, self.position.offset);
    }

    // Use dummy ctx to compute the minimum widget height, max rows, max cols
    // TODO: this is not great
    fn get_dummy_ctx_params(&self, renderer: &Renderer) -> (f32, RenderConstants) {
        let dummy_pos = LayoutPosition {
            offset: [0.0, 1.0],
            max_size: self.position.max_size
        };
        let dummy_ctx = TerminalWidget::new(1, 1, None);
        let rc = dummy_ctx.get_render_constants(renderer, dummy_pos.max_size[0], dummy_pos.max_size[1], self.char_size_px);
        let height = dummy_ctx.get_height_screen(renderer, &dummy_pos, self.char_size_px, self.min_widget_lines);
        (height, rc)
    }

    fn map_onscreen_widgets(
        &mut self,
        renderer: &mut Renderer,
        mut func: impl FnMut(&mut Renderer, &mut TerminalWidget, f32, f32)
    ) {
        let primary_rc = self.context.get_primary_widget().get_render_constants(
            renderer,
            self.position.max_size[0],
            self.position.max_size[1],
            self.char_size_px
        );

        let primary_fullscreen = self.context.get_primary_widget().is_fullscreen();
        let scroll_offset = match primary_fullscreen {
            true => 0.0,
            false => self.scroll_offset * primary_rc.char_size_y_screen
        };

        // Draw visible widgets except the primary
        let top = self.position.offset[1];
        let bottom = self.position.max_size[1] + self.position.offset[1];
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

            let widget_pos = LayoutPosition {
                offset: [self.position.offset[0], start_offset],
                max_size: self.position.max_size
            };

            // Only render if not primary (handled later) and actually visible on screen
            let ctx_height_pre = ctx.get_height_screen(renderer, &widget_pos, self.char_size_px, self.min_widget_lines);
            if !ctx.get_primary() && start_offset - ctx_height_pre < bottom {
                func(renderer, ctx, start_offset, ctx_height_pre);

                // Do some math to determine if the height changed during render. If this happens,
                // it means the widget was reflowed and has a different height now. In order to maintain
                // visual consistency, adjust the scroll offset to reflect this difference iff the widget is
                // not at the top of the screen (i.e. when there are widgets above that would be affected by the height
                // difference). This approach lets us perform the expensive reflow in a deferred manner only
                // when the widget is rendered rather than doing them all at once when the screen is resized.
                let ctx_height_post = ctx.get_height_screen(renderer, &widget_pos, self.char_size_px, self.min_widget_lines);
                let height_diff = ctx_height_post - ctx_height_pre;
                let is_not_top_widget = start_offset - ctx_height_pre > top;
                if height_diff != 0.0 && is_not_top_widget {
                    self.scroll_offset = (self.scroll_offset - (height_diff / primary_rc.char_size_y_screen)).min(0.0)
                }
            }

            cur_offset -= ctx_height_pre;
        }

        // Last (primary) widget is always rendered at the bottom
        // It should snap to the bottom of the last widget when scrolling
        // such that it grows when scrolling down and shrinks when scrolling up,
        // up to a minimum size

        let primary_pos = LayoutPosition {
            offset: [self.position.offset[0], bottom],
            max_size: self.position.max_size
        };
        let (min_height, _) = self.get_dummy_ctx_params(renderer);
        let primary_ctx = self.context.get_primary_widget_mut();
        let primary_height = primary_ctx.get_height_screen(renderer, &primary_pos, self.char_size_px, self.min_widget_lines);
        let start_offset = (bottom - scroll_offset).min(bottom + primary_height - min_height);
        func(renderer, primary_ctx, start_offset, primary_height);
    }
}
