use cel_renderer::renderer::{RenderStats, Renderer};

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
    width_screen: f32,
    height_screen: f32,
    offset_x_screen: f32,
    offset_y_screen: f32,
    can_scroll_up: bool,
    scroll_offset: f32,
    context: TerminalContext,

    last_num_onscreen_widgets: u32,

    widget_height_lines: f32,
}

impl Layout {
    pub fn new(width_screen: f32, height_screen: f32) -> Self {
        Self {
            width_screen,
            height_screen,
            offset_x_screen: 0.0,
            offset_y_screen: 0.0,

            can_scroll_up: false,
            scroll_offset: 0.0,
            context: TerminalContext::new(),

            last_num_onscreen_widgets: 0,

            widget_height_lines: 5.0
        }
    }

    pub fn update(&mut self, input: Option<&mut Input>) -> bool {
        let mut any_event = false;

        let mut input = input;
        any_event |= self.context.update(input.as_deref_mut());

        if self.context.just_split() {
            self.scroll_offset = 0.0;
        }

        // Update scroll
        if let Some(input) = input {
            //let speed_factor = 1.0;
            let speed_factor = 0.01;
            let scroll = input.get_scroll_delta()[1];
            if scroll < 0.0 || self.can_scroll_up {
                if scroll < 0.0 {
                    any_event |= true;
                }
                self.scroll_offset = (self.scroll_offset - scroll * speed_factor).min(0.0);
            }
        }

        any_event
    }

    // Returns true if a rerender should occur after this one
    pub fn render(
        &mut self,
        bg_color: Option<[f32; 4]>,
        divider_color: Option<[f32; 4]>,
        err_bg_color: Option<[f32; 4]>,
        err_divider_color: Option<[f32; 4]>,
        char_size_px: f32,
        renderer: &mut Renderer,
        input: &Input
    ) -> bool {
        let height_screen = self.height_screen;
        let offset_x_screen = self.offset_x_screen;
        let widget_height = self.widget_height_lines;

        // Reset all render states
        for ctx in self.context.get_widgets_mut().iter_mut() {
            // Check if the widget was just closed. If so, update scroll offset
            // for visual consistency
            if ctx.get_just_closed() {
                self.scroll_offset = (self.scroll_offset + ctx.get_last_computed_height_screen()).min(0.0);
            }
            ctx.reset_render_state();
        }

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
        self.map_onscreen_widgets(|ctx, local_offset, _global_offset| {
            let min_widget_height = widget_height * ctx.get_last_line_height_screen();
            let max_size = match ctx.get_expanded() {
                true => height_screen,
                false => min_widget_height,
            };

            min_local_offset = min_local_offset.min(local_offset);

            let (bg_color, divider_color) = match ctx.get_exit_code() {
                None | Some(0) => (bg_color, divider_color),
                // Error code
                _ => (err_bg_color, err_divider_color)
            };

            // Render terminal widget
            should_rerender |= ctx.render(
                renderer,
                input,
                &LayoutPosition {
                    offset: [offset_x_screen, local_offset],
                    max_size: [1.0, max_size],
                },
                char_size_px,
                min_widget_height,
                bg_color,
                divider_color
            );

            count += 1;
        });

        renderer.disable_scissor();

        // Lock scrolling to the last widget
        let top = self.offset_y_screen;
        self.can_scroll_up = min_local_offset < top;
        self.last_num_onscreen_widgets = count;

        should_rerender
    }

    pub fn resize(&mut self, new_size_screen: [f32; 2], new_offset_screen: [f32; 2]) {
        self.width_screen = new_size_screen[0];
        self.height_screen = new_size_screen[1];
        self.offset_x_screen = new_offset_screen[0];
        self.offset_y_screen = new_offset_screen[1];
    }

    pub fn set_current_directory(&mut self, dir: String) {
        self.context.set_current_directory(dir);
    }

    pub fn get_current_directory(&self) -> &str {
        self.context.get_primary_widget().get_current_dir()
    }

    pub fn get_name(&self) -> &str {
        self.get_current_directory()
    }

    pub fn get_debug_lines(&self) -> Vec<String> {
        let widgets = self.context.get_widgets();

        // Accumulate render stats
        let mut stats: RenderStats = Default::default();
        for widget in widgets {
            let w_stat = widget.get_last_render_stats();
            stats.num_fg_instances += w_stat.num_fg_instances;
            stats.num_bg_instances += w_stat.num_bg_instances;
            stats.wrapped_line_count += w_stat.wrapped_line_count;
            stats.rendered_line_count += w_stat.rendered_line_count;
        }

        let active = widgets.last().unwrap();
        let mut text_lines = vec![
            format!("Total widgets: {}", widgets.len()),
            format!("Rendered widgets: {}", self.last_num_onscreen_widgets),
            format!("Total fg quads: {}", stats.num_fg_instances),
            format!("Total bg quads: {}", stats.num_bg_instances),
            format!("Total rendered lines: {}", stats.rendered_line_count),
            format!("Total wrapped lines: {}", stats.wrapped_line_count),
            format!("Scroll offset: {}", self.scroll_offset),
            String::from("\n"),
            format!("Active Widget"),
        ];

        text_lines.extend(active.get_debug_lines());

        text_lines
    }

    fn map_onscreen_widgets(
        &mut self,
        mut func: impl FnMut(&mut TerminalWidget, f32, f32)
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

            if ctx.get_closed() || ctx.is_empty() {
                continue;
            }

            let last_height = ctx.get_last_computed_height_screen();
            let start_offset = cur_offset - scroll_offset - last_height;

            if !ctx.get_primary() && start_offset < bottom {
                func(ctx, start_offset, cur_offset);
            }

            let end_offset = start_offset + last_height;
            if end_offset <= top {
                break;
            }

            cur_offset -= last_height;
        }

        // Last (primary) widget is always rendered at the bottom
        // It should snap to the bottom of the last widget when scrolling
        // such that it grows when scrolling down and shrinks when scrolling up,
        // up to a minimum size
        let last_widget = self.context.get_widgets_mut().last_mut().unwrap();
        let min_widget_height = self.widget_height_lines * last_widget.get_last_line_height_screen();
        let start = bottom - last_widget.get_last_computed_height_screen();
        let start = (start - scroll_offset).min(bottom - min_widget_height);
        func(last_widget, start, bottom);
    }
}
