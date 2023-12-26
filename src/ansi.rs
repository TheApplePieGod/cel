use vte::{Params, Parser, Perform, ParamsIter};
use std::fmt;

use crate::font::{Font, RenderType};
use crate::renderer::{RenderState, Vertex};
use crate::util::Util;

#[derive(Default)]
struct ColorState {
    foreground: Option<[f32; 3]>,
    background: Option<[f32; 3]>
}

#[derive(Default)]
struct PerformState {
    color_state: ColorState,
    x_pos: f32,
    y_pos: f32,
    line_char_count: u32
}

#[derive(Default)]
struct Performer {
    pub render_state: RenderState,
    pub perform_state: PerformState
}

pub struct AnsiHandler {
    performer: Performer,
    state_machine: Parser
}

impl AnsiHandler {
    pub fn new() -> Self {
        Self {
            performer: Default::default(),
            state_machine: Parser::new()
        }
    }

    pub fn handle_sequence(&mut self, seq: &Vec<String>) {
        self.performer.reset_state();
        for string in seq {
            for c in string.bytes() {
                self.state_machine.advance(&mut self.performer, c);
            }
        }
    }

    pub fn get_render_state(&self) -> &RenderState {
        &self.performer.render_state
    }

    pub fn get_render_state_mut(&mut self) -> &mut RenderState {
        &mut self.performer.render_state
    }

}

impl Performer {
    pub fn reset_state(&mut self) {
        self.perform_state = Default::default();

        // Default cursor pos 
        self.perform_state.x_pos = self.render_state.base_x;
        self.perform_state.y_pos = self.render_state.base_y;
    }

    fn parse_16_bit_color(&self, bold: bool, code: u16) -> [f32; 3] {
        let factor: f32 = match bold {
            true => 1.0,
            false => 0.5
        };
        let one = (code & 1) as f32 * factor;
        let two = ((code & 2) >> 1) as f32 * factor;
        let four = ((code & 4) >> 2) as f32 * factor;
        match code {
            1..=6 => [one, two, four],
            0     => match bold {
                true => [0.5, 0.5, 0.5],
                false => [0.0, 0.0, 0.0]
            },
            7     => match bold {
                true => [1.0, 1.0, 1.0],
                false => [0.75, 0.75, 0.75]
            }
            _ => [0.0, 0.0, 0.0]
        }
    }

    fn parse_color_escape(&self, params: ParamsIter) -> ColorState {
        let mut state: ColorState = Default::default();

        let mut is_bold = false;
        for param in params {
            for code in param {
                match code {
                    1 => is_bold = true,
                    30..=37 => state.foreground = Some(self.parse_16_bit_color(is_bold, code - 30)),
                    40..=47 => state.background = Some(self.parse_16_bit_color(is_bold, code - 40)),
                    90..=97   => state.foreground = Some(self.parse_16_bit_color(true, code - 90)),
                    100..=107 => state.background = Some(self.parse_16_bit_color(true, code - 100)),
                    38 => state.foreground = None,
                    39 => state.background = None,
                    _ => {}
                }
            }
        }

        state
    }
}

impl Perform for Performer {
    fn print(&mut self, c: char) {
        let state = &mut self.perform_state;
        let render_state = &mut self.render_state;
        let face_metrics = &render_state.face_metrics;
        let x = &mut state.x_pos;
        let y = &mut state.y_pos;
        let mut font = (**render_state.font.as_ref().unwrap()).borrow_mut();
        if c.is_whitespace() {
            state.line_char_count += 1;
            match c {
                ' ' => *x += face_metrics.space_size,
                '\t' => *x += face_metrics.space_size * font.get_tab_width(),
                _ => {}
            }

            return;
        }

        //let should_wrap = self.render_state.wrap && *x >= self.render_state.chars_per_line as f32 - 1.0;
        let should_wrap = render_state.wrap && state.line_char_count >= render_state.chars_per_line;
        if should_wrap {
            state.line_char_count = 0;
            *x = render_state.base_x;
            *y -= face_metrics.height;
        }

        //TODO: precompute in metrics
        // UV.y is flipped since the underlying atlas bitmaps have flipped y
        let glyph_metrics = &font.get_glyph_data(c);
        let glyph_bound = &glyph_metrics.glyph_bound;
        let atlas_bound = &glyph_metrics.atlas_bound;
        let uv = Font::get_atlas_texcoord(glyph_metrics.atlas_index);
        let uv_min = [
            uv[0] + atlas_bound.left * render_state.coord_scale,
            uv[1] + atlas_bound.top * render_state.coord_scale
        ];
        let uv_max = [
            uv_min[0] + atlas_bound.width() * render_state.coord_scale,
            uv_min[1] - atlas_bound.height() * render_state.coord_scale
        ];

        // TODO: store in separate buffer?
        let fg_color = state.color_state.foreground.as_ref().unwrap_or(&[1.0, 1.0, 1.0]);
        let bg_color = state.color_state.background.as_ref().unwrap_or(&[0.0, 0.0, 0.0]);
        let color = [
            Util::pack_floats(fg_color[0], bg_color[0]),
            Util::pack_floats(fg_color[1], bg_color[1]),
            Util::pack_floats(fg_color[2], bg_color[2])
        ];

        let tr = Vertex {
            position: Util::pack_floats(*x + glyph_bound.right, *y + glyph_bound.top),
            tex_coord: Util::pack_floats(uv_max[0], uv_min[1]),
            color
        };
        let br = Vertex {
            position: Util::pack_floats(*x + glyph_bound.right, *y + glyph_bound.bottom),
            tex_coord: Util::pack_floats(uv_max[0], uv_max[1]),
            color
        };
        let bl = Vertex {
            position: Util::pack_floats(*x + glyph_bound.left, *y + glyph_bound.bottom),
            tex_coord: Util::pack_floats(uv_min[0], uv_max[1]),
            color
        };
        let tl = Vertex {
            position: Util::pack_floats(*x + glyph_bound.left, *y + glyph_bound.top),
            tex_coord: Util::pack_floats(uv_min[0], uv_min[1]),
            color
        };

        let push_vec = match glyph_metrics.render_type {
            RenderType::MSDF => &mut render_state.msdf_vertices,
            RenderType::RASTER => &mut render_state.raster_vertices
        };

        push_vec.push(tl);
        push_vec.push(br);
        push_vec.push(tr);
        push_vec.push(tl);
        push_vec.push(bl);
        push_vec.push(br);

        *x += glyph_metrics.advance;
        state.line_char_count += 1;
    }

    fn execute(&mut self, byte: u8) {
        //println!("[execute] {:02x}", byte);
        match byte {
            b'\n' => {
                self.perform_state.line_char_count = 0;
                self.perform_state.x_pos = self.render_state.base_x;
                self.perform_state.y_pos -= self.render_state.face_metrics.height;
            },
            _ => {}
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        /*
        println!(
            "[hook] params={:?}, intermediates={:?}, ignore={:?}, char={:?}",
            params, intermediates, ignore, c
        );
        */
    }

    fn put(&mut self, byte: u8) {
        //println!("[put] {:02x}", byte);
    }

    fn unhook(&mut self) {
        //println!("[unhook]");
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        //println!("[osc_dispatch] params={:?} bell_terminated={}", params, bell_terminated);
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, c: char) {
        /*
        println!(
            "[csi_dispatch] params={:#?}, intermediates={:?}, ignore={:?}, char={:?}",
            params, intermediates, ignore, c
        );
        */
        match c {
            'm' => {
                self.perform_state.color_state = self.parse_color_escape(params.iter());
                //log::debug!("{:?}", self.color_state);
            },
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        /*
        println!(
            "[esc_dispatch] intermediates={:?}, ignore={:?}, byte={:02x}",
            intermediates, ignore, byte
        );
        */
    }
}

impl fmt::Debug for ColorState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "ColorState: ")?;
        match self.foreground {
            Some(c) => write!(f, "FG<{}, {}, {}>, ", c[0], c[1], c[2])?,
            None => write!(f, "FG<None>")?
        };
        match self.background {
            Some(c) => write!(f, "BG<{}, {}, {}>, ", c[0], c[1], c[2])?,
            None => write!(f, "BG<None>")?
        };

        Ok(())
    }
}
